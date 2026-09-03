//! Remote target model and `~/.shannon/remotes.toml` persistence.
//!
//! A target is a first-class execution environment: an SSH host or a Docker
//! container (optionally reached through an SSH host). Credentials are never
//! stored here — authentication is delegated entirely to the system ssh
//! (config / agent / known_hosts).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Kind of remote execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    /// Remote host reached over system ssh.
    Ssh,
    /// Running Docker container reached via `docker exec`.
    Docker,
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetKind::Ssh => f.write_str("ssh"),
            TargetKind::Docker => f.write_str("docker"),
        }
    }
}

/// One remote execution target (`[[targets]]` in remotes.toml).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteTarget {
    /// Unique, user-chosen name (used by `--target`, `/remote use`).
    pub name: String,
    pub kind: TargetKind,
    /// SSH host: host alias from `~/.ssh/config` or `user@host`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// SSH port override; normally left to the ssh config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// SSH user override; normally left to the ssh config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Docker: running container name or ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Docker: shell used to compose in-container commands (default `sh`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Docker: route `docker exec` through this SSH target (remote Docker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_target: Option<String>,
    /// Remote workspace root: sandbox root + default cwd for the session.
    pub workspace_dir: PathBuf,
}

/// Validation failure with a stable, user-presentable reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(pub &'static str);

impl RemoteTarget {
    /// SSH destination string (`user@host`) or plain host alias.
    pub fn ssh_destination(&self) -> String {
        let host = self.host.clone().unwrap_or_default();
        match &self.user {
            Some(u) if !host.contains('@') => format!("{u}@{host}"),
            _ => host,
        }
    }

    /// Docker in-container shell (default `sh`).
    pub fn docker_shell(&self) -> &str {
        self.shell.as_deref().unwrap_or("sh")
    }

    /// Structural validation applied before persistence and use.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError("target name must not be empty"));
        }
        if !self.workspace_dir.is_absolute() {
            return Err(ValidationError("workspace_dir must be an absolute path"));
        }
        match self.kind {
            TargetKind::Ssh => {
                if self.host.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(ValidationError("ssh target requires host"));
                }
            }
            TargetKind::Docker => {
                if self.container.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(ValidationError("docker target requires container"));
                }
            }
        }
        Ok(())
    }
}

/// Parsed `remotes.toml` document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemotesFile {
    /// Target used when nothing more specific selects one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RemoteTarget>,
}

impl RemotesFile {
    /// Parse a remotes document from bytes.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Load from `path`; a missing file yields the empty document.
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Load from the default location (`~/.shannon/remotes.toml`).
    pub fn load_default() -> Self {
        Self::load(&remotes_path()).unwrap_or_default()
    }

    /// Persist to `path` with 0600 permissions on Unix.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Create with restrictive mode from the start (no permission window).
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            f.write_all(body.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(path, body)?;
        }
        Ok(())
    }

    /// Look up a target by name.
    pub fn resolve(&self, name: &str) -> Option<&RemoteTarget> {
        self.targets.iter().find(|t| t.name == name)
    }

    /// Resolve the active target: CLI > env (`SHANNON_TARGET`) > default_target.
    /// Returns `None` when no target is configured (local world).
    pub fn resolve_active(&self, cli: Option<&str>) -> Option<RemoteTarget> {
        let from_env = std::env::var("SHANNON_TARGET").ok();
        let name = match cli.map(str::trim).filter(|s| !s.is_empty()) {
            Some(name) => name,
            None => match from_env.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(name) => name,
                None => self.default_target.as_deref()?,
            },
        };
        self.resolve(name).cloned()
    }

    /// Add or replace a target by name, validating first.
    pub fn upsert(&mut self, target: RemoteTarget) -> Result<(), ValidationError> {
        target.validate()?;
        self.targets.retain(|t| t.name != target.name);
        self.targets.push(target);
        Ok(())
    }

    /// Remove a target by name; returns whether anything was removed.
    /// Clears `default_target` when it pointed at the removed target.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.targets.len();
        self.targets.retain(|t| t.name != name);
        if self.default_target.as_deref() == Some(name) {
            self.default_target = None;
        }
        self.targets.len() != before
    }
}

/// Default remotes file location: `~/.shannon/remotes.toml`.
pub fn remotes_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".shannon")
        .join("remotes.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
default_target = "build-box"

[[targets]]
name = "build-box"
kind = "ssh"
host = "build-box"
port = 22
workspace_dir = "/home/ed/proj"

[[targets]]
name = "ci-runner"
kind = "docker"
container = "shannon-ci"
shell = "bash"
ssh_target = "build-box"
workspace_dir = "/workspace"
"#;

    fn ssh_target() -> RemoteTarget {
        RemoteTarget {
            name: "build-box".into(),
            kind: TargetKind::Ssh,
            host: Some("build-box".into()),
            port: Some(22),
            user: None,
            container: None,
            shell: None,
            ssh_target: None,
            workspace_dir: PathBuf::from("/home/ed/proj"),
        }
    }

    fn docker_target() -> RemoteTarget {
        RemoteTarget {
            name: "ci-runner".into(),
            kind: TargetKind::Docker,
            host: None,
            port: None,
            user: None,
            container: Some("shannon-ci".into()),
            shell: Some("bash".into()),
            ssh_target: Some("build-box".into()),
            workspace_dir: PathBuf::from("/workspace"),
        }
    }

    #[test]
    fn toml_roundtrip_preserves_all_fields() {
        let parsed = RemotesFile::parse(FIXTURE).expect("parses");
        assert_eq!(parsed.default_target.as_deref(), Some("build-box"));
        assert_eq!(parsed.targets.len(), 2);
        assert_eq!(parsed.targets[0], ssh_target());
        assert_eq!(parsed.targets[1], docker_target());

        let re = RemotesFile::parse(&toml::to_string_pretty(&parsed).unwrap()).unwrap();
        assert_eq!(re, parsed);
    }

    #[test]
    fn resolve_active_priority_cli_over_env_over_default() {
        let file = RemotesFile::parse(FIXTURE).unwrap();
        // default_target wins with nothing else set
        assert_eq!(
            file.resolve_active(None).unwrap().name,
            "build-box"
        );
        // CLI beats default
        assert_eq!(
            file.resolve_active(Some("ci-runner")).unwrap().name,
            "ci-runner"
        );
        // Unknown CLI name -> no target (fail loudly, not silently local)
        assert!(file.resolve_active(Some("nope")).is_none());
    }

    #[test]
    fn resolve_active_empty_cli_string_falls_through() {
        let file = RemotesFile::parse(FIXTURE).unwrap();
        assert_eq!(file.resolve_active(Some("  ")).unwrap().name, "build-box");
    }

    #[test]
    fn docker_target_requires_container_and_absolute_workspace() {
        let mut t = docker_target();
        t.container = None;
        assert_eq!(
            t.validate(),
            Err(ValidationError("docker target requires container"))
        );

        t.container = Some("c".into());
        t.workspace_dir = PathBuf::from("relative/path");
        assert_eq!(
            t.validate(),
            Err(ValidationError("workspace_dir must be an absolute path"))
        );
    }

    #[test]
    fn ssh_target_requires_host() {
        let mut t = ssh_target();
        t.host = None;
        assert_eq!(t.validate(), Err(ValidationError("ssh target requires host")));
    }

    #[test]
    fn ssh_destination_prefers_explicit_user() {
        let mut t = ssh_target();
        assert_eq!(t.ssh_destination(), "build-box");
        t.user = Some("ed".into());
        assert_eq!(t.ssh_destination(), "ed@build-box");
        // user@host already carries the user
        t.host = Some("ed@elsewhere".into());
        t.user = Some("ignored".into());
        assert_eq!(t.ssh_destination(), "ed@elsewhere");
    }

    #[test]
    fn save_sets_0600_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("remotes.toml");
        let file = RemotesFile::parse(FIXTURE).unwrap();
        file.save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // Content round-trips
        assert_eq!(RemotesFile::load(&path).unwrap(), file);
    }

    #[test]
    fn load_missing_file_yields_empty_document() {
        let tmp = tempfile::tempdir().unwrap();
        let file = RemotesFile::load(&tmp.path().join("absent.toml")).unwrap();
        assert_eq!(file, RemotesFile::default());
        assert!(file.resolve_active(None).is_none());
    }

    #[test]
    fn remove_clears_default_target_reference() {
        let mut file = RemotesFile::parse(FIXTURE).unwrap();
        assert!(file.remove("build-box"));
        assert_eq!(file.default_target, None);
        assert!(!file.remove("build-box"));
    }

    #[test]
    fn upsert_replaces_same_name_and_validates() {
        let mut file = RemotesFile::parse(FIXTURE).unwrap();
        let mut t = ssh_target();
        t.port = Some(2222);
        file.upsert(t).unwrap();
        assert_eq!(file.targets.len(), 2);
        assert_eq!(file.resolve("build-box").unwrap().port, Some(2222));

        let mut bad = ssh_target();
        bad.name = "  ".into();
        assert!(file.upsert(bad).is_err());
    }

    #[test]
    fn docker_shell_defaults_to_sh() {
        let mut t = docker_target();
        assert_eq!(t.docker_shell(), "bash");
        t.shell = None;
        assert_eq!(t.docker_shell(), "sh");
    }
}
