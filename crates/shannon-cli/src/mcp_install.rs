//! `.mcpb` bundle installation.
//!
//! An `.mcpb` file is a zip archive containing an `.mcp.json` at the root.
//! Installing it extracts the server definitions and merges them into either
//! the project's `.mcp.json` or the user's `~/.shannon/settings.json`.
//!
//! ## Security
//!
//! A `.mcpb` bundle can install arbitrary MCP server commands. To make that
//! explicit and tamper-evident:
//!
//! - `preview_bundle()` returns what would be installed without writing.
//! - `install_bundle()` refuses to follow symlinks for the target path.
//! - Manifests larger than [`MAX_MANIFEST_SIZE`] are rejected.
//! - An existing settings file that fails to parse as JSON aborts the install
//!   rather than being silently reset.

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Upper bound on the uncompressed `.mcp.json` size we are willing to read.
const MAX_MANIFEST_SIZE: u64 = 10 * 1024 * 1024;

/// Where to install the bundle's servers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallScope {
    /// Project-level `.mcp.json` (shared with team via version control).
    Project,
    /// User-level `~/.shannon/settings.json` (private, not shared).
    User,
}

/// Errors that can occur during bundle installation.
#[derive(Debug)]
pub enum InstallError {
    Io(std::io::Error),
    Zip(String),
    InvalidManifest(String),
    InvalidSettings(String),
    SettingsWrite(String),
    SymlinkTarget(PathBuf),
    ManifestTooLarge { size: u64, limit: u64 },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Zip(e) => write!(f, "zip error: {e}"),
            Self::InvalidManifest(e) => write!(f, "invalid manifest: {e}"),
            Self::InvalidSettings(e) => write!(f, "existing settings file is invalid: {e}"),
            Self::SettingsWrite(e) => write!(f, "settings write error: {e}"),
            Self::SymlinkTarget(p) => write!(
                f,
                "refusing to write through symlink {} for security",
                p.display()
            ),
            Self::ManifestTooLarge { size, limit } => {
                write!(f, ".mcp.json is {size} bytes, exceeds {limit}-byte limit")
            }
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for InstallError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidManifest(format!("JSON error: {e}"))
    }
}

/// Result of a successful install.
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Number of servers added or updated.
    pub servers_installed: usize,
    /// Path to the settings file that was modified.
    pub target_path: PathBuf,
    /// Names of the servers that were installed.
    pub server_names: Vec<String>,
}

/// Summary of a server entry's command, extracted best-effort from JSON.
#[derive(Debug, Clone)]
pub struct ServerCommandSummary {
    pub command: String,
    pub args: Vec<String>,
}

/// Per-server preview entry.
#[derive(Debug, Clone)]
pub struct ServerPreview {
    pub name: String,
    pub command: Option<ServerCommandSummary>,
    pub overwrites_existing: bool,
}

/// Preview of an install without writing anything.
#[derive(Debug, Clone)]
pub struct InstallPreview {
    pub scope: InstallScope,
    pub target_path: PathBuf,
    pub servers: Vec<ServerPreview>,
}

impl InstallPreview {
    pub fn has_overwrite(&self) -> bool {
        self.servers.iter().any(|s| s.overwrites_existing)
    }
}

/// Preview a bundle install: reads the manifest, resolves the target path,
/// and returns what would be installed without touching the filesystem
/// (beyond opening the bundle).
pub fn preview_bundle(
    bundle_path: &Path,
    scope: InstallScope,
) -> Result<InstallPreview, InstallError> {
    let file = fs::File::open(bundle_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| InstallError::Zip(format!("failed to open bundle: {e}")))?;

    let mcp_json = read_mcp_json(&mut archive)?;
    let target = resolve_target(scope)?;

    let existing_servers = existing_server_names(&target)?;
    let new_servers = mcp_json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            InstallError::InvalidManifest(".mcp.json has no 'mcpServers' object".into())
        })?;

    if new_servers.is_empty() {
        return Err(InstallError::InvalidManifest(
            ".mcp.json 'mcpServers' is empty".into(),
        ));
    }

    let mut servers = Vec::with_capacity(new_servers.len());
    for (name, config) in new_servers {
        servers.push(ServerPreview {
            name: name.clone(),
            command: summarize_command(config),
            overwrites_existing: existing_servers.contains(name.as_str()),
        });
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(InstallPreview {
        scope,
        target_path: target,
        servers,
    })
}

/// Install a `.mcpb` bundle by extracting its `.mcp.json` and merging
/// the servers into the chosen scope's settings file.
///
/// Callers writing a CLI should call [`preview_bundle`] first to show the
/// user what will be installed and obtain confirmation — `.mcpb` bundles
/// can install arbitrary commands.
pub fn install_bundle(
    bundle_path: &Path,
    scope: InstallScope,
) -> Result<InstallResult, InstallError> {
    let file = fs::File::open(bundle_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| InstallError::Zip(format!("failed to open bundle: {e}")))?;

    let mcp_json = read_mcp_json(&mut archive)?;

    let target = resolve_target(scope)?;
    reject_symlink_target(&target)?;

    let (count, names) = merge_mcp_servers(&target, &mcp_json)?;

    Ok(InstallResult {
        servers_installed: count,
        target_path: target,
        server_names: names,
    })
}

fn resolve_target(scope: InstallScope) -> Result<PathBuf, InstallError> {
    match scope {
        InstallScope::User => user_settings_path(),
        InstallScope::Project => Ok(project_mcp_json_path()),
    }
}

/// Refuse to write through a symlink — an attacker who can plant a symlink
/// at `.mcp.json` or `~/.shannon/settings.json` could otherwise redirect
/// writes to arbitrary files (e.g., `~/.bashrc`).
fn reject_symlink_target(target: &Path) -> Result<(), InstallError> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(InstallError::SymlinkTarget(target.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(InstallError::Io(e)),
    }
}

/// Best-effort extraction of `command` / `args` from a server config for
/// display in a preview. Returns `None` if the config doesn't look like a
/// server definition we can summarize.
fn summarize_command(config: &serde_json::Value) -> Option<ServerCommandSummary> {
    let obj = config.as_object()?;
    let command = obj.get("command").and_then(|v| v.as_str())?.to_string();
    let args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(ServerCommandSummary { command, args })
}

/// Names of servers already present in the target's existing `mcpServers`.
/// Returns an empty set if the target does not exist. Returns an error if
/// the target exists but cannot be parsed as JSON.
fn existing_server_names(
    target: &Path,
) -> Result<std::collections::BTreeSet<String>, InstallError> {
    if !target.exists() {
        return Ok(Default::default());
    }
    let content = fs::read_to_string(target)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| InstallError::InvalidSettings(format!("{}: {e}", target.display())))?;
    let names = value
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    Ok(names)
}

/// Read and parse `.mcp.json` from a zip archive.
fn read_mcp_json<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<serde_json::Value, InstallError> {
    let name = if archive.by_name(".mcp.json").is_ok() {
        ".mcp.json"
    } else if archive.by_name("mcp.json").is_ok() {
        "mcp.json"
    } else {
        return Err(InstallError::InvalidManifest(
            "bundle does not contain .mcp.json".into(),
        ));
    };

    let mut entry = archive
        .by_name(name)
        .map_err(|e| InstallError::InvalidManifest(format!("failed to read {name}: {e}")))?;

    if entry.size() > MAX_MANIFEST_SIZE {
        return Err(InstallError::ManifestTooLarge {
            size: entry.size(),
            limit: MAX_MANIFEST_SIZE,
        });
    }

    let mut content = String::new();
    entry.read_to_string(&mut content)?;

    serde_json::from_str(&content)
        .map_err(|e| InstallError::InvalidManifest(format!(".mcp.json is not valid JSON: {e}")))
}

/// Merge servers from `new_json` into the settings file at `target`.
///
/// Creates the file if it doesn't exist. Preserves existing servers and
/// other keys. Returns the count and names of installed servers. Returns
/// an error if the target exists but cannot be parsed as JSON.
fn merge_mcp_servers(
    target: &Path,
    new_json: &serde_json::Value,
) -> Result<(usize, Vec<String>), InstallError> {
    let new_servers = new_json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            InstallError::InvalidManifest(".mcp.json has no 'mcpServers' object".into())
        })?;

    if new_servers.is_empty() {
        return Err(InstallError::InvalidManifest(
            ".mcp.json 'mcpServers' is empty".into(),
        ));
    }

    let mut existing: serde_json::Value = if target.exists() {
        let content = fs::read_to_string(target)?;
        serde_json::from_str(&content)
            .map_err(|e| InstallError::InvalidSettings(format!("{}: {e}", target.display())))?
    } else {
        serde_json::json!({})
    };

    let root = existing
        .as_object_mut()
        .ok_or_else(|| InstallError::SettingsWrite("settings root is not an object".into()))?;

    let servers_entry = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| serde_json::json!({}));

    let servers_obj = servers_entry
        .as_object_mut()
        .ok_or_else(|| InstallError::SettingsWrite("'mcpServers' is not an object".into()))?;

    let mut names = Vec::with_capacity(new_servers.len());
    for (name, config) in new_servers {
        servers_obj.insert(name.clone(), config.clone());
        names.push(name.clone());
    }
    names.sort();

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let pretty = serde_json::to_string_pretty(&existing)?;
    let mut file = fs::File::create(target)?;
    file.write_all(pretty.as_bytes())?;
    file.write_all(b"\n")?;

    Ok((new_servers.len(), names))
}

/// Path to the project-level `.mcp.json` (current directory).
fn project_mcp_json_path() -> PathBuf {
    PathBuf::from(".mcp.json")
}

/// Path to `~/.shannon/settings.json`, creating the directory if needed.
fn user_settings_path() -> Result<PathBuf, InstallError> {
    let home = dirs::home_dir()
        .ok_or_else(|| InstallError::SettingsWrite("cannot determine home directory".into()))?;
    let dir = home.join(".shannon");
    Ok(dir.join("settings.json"))
}

/// Build an in-memory `.mcpb` zip from a JSON value for testing.
#[cfg(test)]
fn build_test_bundle(mcp_json: &serde_json::Value) -> Vec<u8> {
    use std::io::Cursor;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file(".mcp.json", options).unwrap();
    let json_str = serde_json::to_string_pretty(mcp_json).unwrap();
    zip.write_all(json_str.as_bytes()).unwrap();
    zip.finish().unwrap();

    buf.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_bundle(dir: &Path, json: &serde_json::Value) -> PathBuf {
        let bundle = build_test_bundle(json);
        let path = dir.join("test.mcpb");
        fs::write(&path, &bundle).unwrap();
        path
    }

    #[test]
    fn install_to_project_creates_mcp_json() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"]
                }
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project).unwrap();
        assert_eq!(result.servers_installed, 1);
        assert_eq!(result.server_names, vec!["github"]);

        let written = fs::read_to_string(".mcp.json").unwrap();
        assert!(written.contains("github"));
        assert!(written.contains("@modelcontextprotocol/server-github"));
    }

    #[test]
    fn install_preserves_existing_servers() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        fs::write(
            ".mcp.json",
            r#"{"mcpServers": {"existing": {"command": "old"}}}"#,
        )
        .unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "new": {"command": "new"}
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project).unwrap();
        assert_eq!(result.servers_installed, 1);

        let written = fs::read_to_string(".mcp.json").unwrap();
        assert!(written.contains("existing"));
        assert!(written.contains("old"));
        assert!(written.contains("new"));
    }

    #[test]
    fn install_overwrites_same_name_server() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        fs::write(
            ".mcp.json",
            r#"{"mcpServers": {"github": {"command": "old", "args": []}}}"#,
        )
        .unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "github": {"command": "new", "args": ["--updated"]}
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        install_bundle(&bundle, InstallScope::Project).unwrap();

        let written = fs::read_to_string(".mcp.json").unwrap();
        assert!(written.contains("new"));
        assert!(written.contains("--updated"));
        assert!(!written.contains("\"old\""));
    }

    #[test]
    fn install_rejects_bundle_without_mcp_json() {
        use std::io::Cursor;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let tmp = TempDir::new().unwrap();
        let mut buf = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut buf);
        zip.start_file("README.md", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not a config").unwrap();
        zip.finish().unwrap();

        let path = tmp.path().join("bad.mcpb");
        fs::write(&path, buf.into_inner()).unwrap();

        let result = install_bundle(&path, InstallScope::Project);
        assert!(matches!(result, Err(InstallError::InvalidManifest(_))));
    }

    #[test]
    fn install_rejects_empty_servers() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let json = serde_json::json!({"mcpServers": {}});
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project);
        assert!(matches!(result, Err(InstallError::InvalidManifest(_))));
    }

    #[test]
    fn install_multiple_servers_at_once() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "github": {"command": "gh-server"},
                "fs": {"command": "fs-server"},
                "slack": {"command": "slack-server"}
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project).unwrap();
        assert_eq!(result.servers_installed, 3);
        assert_eq!(result.server_names.len(), 3);
        assert!(result.server_names.contains(&"github".to_string()));
        assert!(result.server_names.contains(&"fs".to_string()));
        assert!(result.server_names.contains(&"slack".to_string()));
    }

    #[test]
    fn install_accepts_mcp_json_without_leading_dot() {
        use std::io::Cursor;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let mut buf = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut buf);
        zip.start_file("mcp.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"mcpServers": {"x": {"command": "y"}}}"#)
            .unwrap();
        zip.finish().unwrap();

        let path = tmp.path().join("alt.mcpb");
        fs::write(&path, buf.into_inner()).unwrap();

        let result = install_bundle(&path, InstallScope::Project).unwrap();
        assert_eq!(result.servers_installed, 1);
    }

    #[test]
    fn install_preserves_non_mcp_keys_in_user_settings() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let user_dir = tmp.path().join(".shannon");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(
            user_dir.join("settings.json"),
            r#"{"theme": "dark", "mcpServers": {"old": {"command": "old"}}}"#,
        )
        .unwrap();

        // SAFETY: tests run single-threaded; no concurrent env access.
        unsafe { std::env::set_var("HOME", tmp.path()) };

        let json = serde_json::json!({
            "mcpServers": {"new": {"command": "new"}}
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::User).unwrap();
        assert_eq!(result.servers_installed, 1);

        let written = fs::read_to_string(user_dir.join("settings.json")).unwrap();
        assert!(written.contains("theme"));
        assert!(written.contains("dark"));
        assert!(written.contains("old"));
        assert!(written.contains("new"));
    }

    #[test]
    fn install_scope_variant_names() {
        assert_eq!(format!("{:?}", InstallScope::Project), "Project");
        assert_eq!(format!("{:?}", InstallScope::User), "User");
    }

    #[test]
    fn install_error_display() {
        let e = InstallError::Zip("boom".into());
        assert_eq!(format!("{e}"), "zip error: boom");

        let e = InstallError::InvalidManifest("bad json".into());
        assert_eq!(format!("{e}"), "invalid manifest: bad json");

        let e = InstallError::SymlinkTarget(PathBuf::from(".mcp.json"));
        assert!(format!("{e}").contains("symlink"));
        assert!(format!("{e}").contains(".mcp.json"));

        let e = InstallError::ManifestTooLarge {
            size: 20_000_000,
            limit: MAX_MANIFEST_SIZE,
        };
        let msg = format!("{e}");
        assert!(msg.contains("20000000"));
        assert!(msg.contains("limit"));

        let e = InstallError::InvalidSettings("boom".into());
        assert!(format!("{e}").contains("existing settings"));
    }

    #[test]
    fn user_settings_path_uses_shannon_subdir() {
        let tmp = TempDir::new().unwrap();
        // SAFETY: tests run single-threaded; no concurrent env access.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let path = user_settings_path().unwrap();
        assert!(path.to_string_lossy().contains(".shannon"));
        assert!(path.to_string_lossy().contains("settings.json"));
    }

    #[test]
    fn project_settings_path_is_relative() {
        let path = project_mcp_json_path();
        assert_eq!(path, PathBuf::from(".mcp.json"));
    }

    // ── security-hardening tests ────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn install_rejects_symlink_target() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Plant a symlink so .mcp.json points elsewhere.
        let elsewhere = tmp.path().join("elsewhere.json");
        symlink(&elsewhere, ".mcp.json").unwrap();

        let json = serde_json::json!({
            "mcpServers": {"x": {"command": "y"}}
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project);
        assert!(
            matches!(result, Err(InstallError::SymlinkTarget(_))),
            "got: {result:?}"
        );

        // Target file must not have been created through the symlink.
        assert!(!elsewhere.exists());
    }

    #[test]
    fn install_rejects_oversized_manifest() {
        use std::io::Cursor;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Build a manifest whose declared size exceeds the cap. We inject
        // padding inside the JSON string value so it parses as JSON but
        // is large.
        let padding = "x".repeat((MAX_MANIFEST_SIZE + 1) as usize);
        let json_str = format!(
            r#"{{"mcpServers": {{ "pad": {{ "command": "true", "args": ["{}"] }} }}}}"#,
            padding
        );

        let mut buf = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut buf);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file(".mcp.json", options).unwrap();
        zip.write_all(json_str.as_bytes()).unwrap();
        zip.finish().unwrap();

        let path = tmp.path().join("big.mcpb");
        fs::write(&path, buf.into_inner()).unwrap();

        let result = install_bundle(&path, InstallScope::Project);
        assert!(
            matches!(result, Err(InstallError::ManifestTooLarge { .. })),
            "got: {result:?}"
        );
    }

    #[test]
    fn install_rejects_unparseable_existing_settings() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        fs::write(".mcp.json", "this is not json {{{").unwrap();

        let json = serde_json::json!({
            "mcpServers": {"new": {"command": "new"}}
        });
        let bundle = write_bundle(tmp.path(), &json);

        let result = install_bundle(&bundle, InstallScope::Project);
        assert!(
            matches!(result, Err(InstallError::InvalidSettings(_))),
            "got: {result:?}"
        );

        // Original file must be preserved byte-for-byte.
        let preserved = fs::read_to_string(".mcp.json").unwrap();
        assert_eq!(preserved, "this is not json {{{");
    }

    #[test]
    fn preview_returns_servers_and_target_without_writing() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "alpha": {"command": "run-a", "args": ["--flag", "value"]},
                "beta": {"command": "run-b"}
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        let preview = preview_bundle(&bundle, InstallScope::Project).unwrap();
        assert_eq!(preview.scope, InstallScope::Project);
        assert_eq!(preview.target_path, PathBuf::from(".mcp.json"));
        assert_eq!(preview.servers.len(), 2);

        let alpha = preview.servers.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.command.as_ref().unwrap().command, "run-a");
        assert_eq!(
            alpha.command.as_ref().unwrap().args,
            vec!["--flag".to_string(), "value".to_string()]
        );
        assert!(!alpha.overwrites_existing);
        assert!(!preview.has_overwrite());

        // Preview must not have created the target.
        assert!(!Path::new(".mcp.json").exists());
    }

    #[test]
    fn preview_flags_overwrites() {
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        fs::write(
            ".mcp.json",
            r#"{"mcpServers": {"github": {"command": "old"}}}"#,
        )
        .unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "github": {"command": "new"},
                "new": {"command": "fresh"}
            }
        });
        let bundle = write_bundle(tmp.path(), &json);

        let preview = preview_bundle(&bundle, InstallScope::Project).unwrap();
        assert!(preview.has_overwrite());

        let github = preview.servers.iter().find(|s| s.name == "github").unwrap();
        assert!(github.overwrites_existing);

        let new = preview.servers.iter().find(|s| s.name == "new").unwrap();
        assert!(!new.overwrites_existing);
    }

    #[cfg(unix)]
    #[test]
    fn preview_does_not_reject_symlink_target() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let elsewhere = tmp.path().join("elsewhere.json");
        symlink(&elsewhere, ".mcp.json").unwrap();

        let json = serde_json::json!({
            "mcpServers": {"x": {"command": "y"}}
        });
        let bundle = write_bundle(tmp.path(), &json);

        // Preview must succeed; only install refuses.
        let preview = preview_bundle(&bundle, InstallScope::Project).unwrap();
        assert_eq!(preview.servers.len(), 1);
    }
}
