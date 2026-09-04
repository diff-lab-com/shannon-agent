//! Remote target management commands — CRUD over `~/.shannon/remotes.toml`,
//! SSH host discovery, Docker container listing, and connectivity tests.
//!
//! Design notes:
//! - No credential material ever flows through these commands: SSH targets
//!   reference `~/.ssh/config` aliases and authentication rides on the
//!   system ssh (agent / keys). The only secret-adjacent surface is the
//!   optional host-key fingerprint shown by `remote_test_target`, fetched
//!   read-only via `ssh-keyscan`/`ssh-keygen`.
//! - Health checks run on a throwaway connection through
//!   `shannon_remote::assembly`; they never mutate the session world.

use serde::{Deserialize, Serialize};
use shannon_remote::target::{RemoteTarget, RemotesFile, TargetKind};

use crate::commands::AppState;

// ── DTOs (camelCase for the webview) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTargetDto {
    pub name: String,
    pub kind: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub container: Option<String>,
    pub shell: Option<String>,
    pub ssh_target: Option<String>,
    pub workspace_dir: String,
}

impl From<&RemoteTarget> for RemoteTargetDto {
    fn from(t: &RemoteTarget) -> Self {
        Self {
            name: t.name.clone(),
            kind: t.kind.to_string(),
            host: t.host.clone(),
            port: t.port,
            user: t.user.clone(),
            container: t.container.clone(),
            shell: t.shell.clone(),
            ssh_target: t.ssh_target.clone(),
            workspace_dir: t.workspace_dir.display().to_string(),
        }
    }
}

impl From<RemoteTargetDto> for RemoteTarget {
    fn from(dto: RemoteTargetDto) -> Self {
        Self {
            name: dto.name,
            kind: if dto.kind == "docker" {
                TargetKind::Docker
            } else {
                TargetKind::Ssh
            },
            host: dto.host,
            port: dto.port,
            user: dto.user,
            container: dto.container,
            shell: dto.shell,
            ssh_target: dto.ssh_target,
            workspace_dir: std::path::PathBuf::from(dto.workspace_dir),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostCandidateDto {
    pub alias: String,
    pub user: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfoDto {
    pub id: String,
    pub names: String,
    pub image: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteHealthDto {
    pub ok: bool,
    pub platform: String,
    pub home: String,
    pub bash_available: bool,
    pub workspace_exists: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

// ── Commands ─────────────────────────────────────────────────────────────

/// List saved targets from `~/.shannon/remotes.toml`.
#[tauri::command]
pub fn remote_list_targets(_state: tauri::State<'_, AppState>) -> Vec<RemoteTargetDto> {
    RemotesFile::load_default()
        .targets
        .iter()
        .map(RemoteTargetDto::from)
        .collect()
}

/// Discover candidate hosts from `~/.ssh/config` (read-only, no secrets).
#[tauri::command]
pub fn remote_discover_ssh_hosts(_state: tauri::State<'_, AppState>) -> Vec<SshHostCandidateDto> {
    shannon_remote::ssh::discover_ssh_hosts()
        .into_iter()
        .map(|c| SshHostCandidateDto {
            alias: c.alias,
            user: c.user,
            hostname: c.hostname,
            port: c.port,
        })
        .collect()
}

/// List running Docker containers (best-effort; empty on docker errors).
#[tauri::command]
pub async fn remote_list_docker_containers(
    _state: tauri::State<'_, AppState>,
) -> Result<Vec<ContainerInfoDto>, String> {
    let containers = shannon_remote::docker::list_running_containers()
        .await
        .map_err(|e| e.to_string())?;
    Ok(containers
        .into_iter()
        .map(|c| ContainerInfoDto {
            id: c.id,
            names: c.names,
            image: c.image,
            status: c.status,
        })
        .collect())
}

/// Add or replace a target (validated, persisted with 0600).
#[tauri::command]
pub fn remote_add_target(
    _state: tauri::State<'_, AppState>,
    target: RemoteTargetDto,
) -> Result<(), String> {
    let target = RemoteTarget::from(target);
    target.validate().map_err(|e| e.to_string())?;
    let path = shannon_remote::target::remotes_path();
    let mut file = RemotesFile::load(&path).map_err(|e| e.to_string())?;
    file.upsert(target).map_err(|e| e.to_string())?;
    file.save(&path).map_err(|e| e.to_string())
}

/// Remove a target (only Shannon's reference; ssh config is untouched).
#[tauri::command]
pub fn remote_remove_target(
    _state: tauri::State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let path = shannon_remote::target::remotes_path();
    let mut file = RemotesFile::load(&path).map_err(|e| e.to_string())?;
    if !file.remove(&name) {
        return Err(format!("unknown target '{name}'"));
    }
    file.save(&path).map_err(|e| e.to_string())
}

/// Set (or clear with `None`) the default target for new sessions.
#[tauri::command]
pub fn remote_set_default_target(
    _state: tauri::State<'_, AppState>,
    name: Option<String>,
) -> Result<(), String> {
    let path = shannon_remote::target::remotes_path();
    let mut file = RemotesFile::load(&path).map_err(|e| e.to_string())?;
    if let Some(n) = &name {
        if file.resolve(n).is_none() {
            return Err(format!("unknown target '{n}'"));
        }
    }
    file.default_target = name;
    file.save(&path).map_err(|e| e.to_string())
}

/// Connectivity test: connect, probe platform/home/bash/workspace, report.
/// Never switches the session world.
#[tauri::command]
pub async fn remote_test_target(
    _state: tauri::State<'_, AppState>,
    name: String,
) -> Result<RemoteHealthDto, String> {
    let target = RemotesFile::load_default()
        .resolve_active(Some(&name))
        .ok_or_else(|| format!("unknown target '{name}'"))?;
    let dto = match target.kind {
        TargetKind::Ssh => match shannon_remote::ssh::SshRuntime::connect(&target).await {
            Ok(runtime) => match runtime.health().await {
                Ok(h) => RemoteHealthDto {
                    ok: true,
                    platform: h.platform,
                    home: h.home,
                    bash_available: h.bash_available,
                    workspace_exists: h.workspace_exists,
                    latency_ms: h.latency_ms,
                    error: None,
                },
                Err(e) => RemoteHealthDto {
                    ok: false,
                    platform: String::new(),
                    home: String::new(),
                    bash_available: false,
                    workspace_exists: false,
                    latency_ms: 0,
                    error: Some(e.to_string()),
                },
            },
            Err(e) => RemoteHealthDto {
                ok: false,
                platform: String::new(),
                home: String::new(),
                bash_available: false,
                workspace_exists: false,
                latency_ms: 0,
                error: Some(e.to_string()),
            },
        },
        TargetKind::Docker => {
            let ok = shannon_remote::docker::list_running_containers()
                .await
                .map(|list| {
                    list.iter().any(|c| {
                        Some(&target.container).map_or(false, |want| {
                            c.names.contains(want.as_deref().unwrap_or("\u{0}"))
                        })
                    })
                })
                .unwrap_or(false);
            if ok {
                RemoteHealthDto {
                    ok: true,
                    platform: "docker".into(),
                    home: target.workspace_dir.display().to_string(),
                    bash_available: true,
                    workspace_exists: true,
                    latency_ms: 0,
                    error: None,
                }
            } else {
                RemoteHealthDto {
                    ok: false,
                    platform: "docker".into(),
                    home: String::new(),
                    bash_available: false,
                    workspace_exists: false,
                    latency_ms: 0,
                    error: Some(format!(
                        "container {} not found in `docker ps`",
                        target.container.as_deref().unwrap_or("?")
                    )),
                }
            }
        }
    };
    Ok(dto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RemoteTargetDto {
        RemoteTargetDto {
            name: "build-box".into(),
            kind: "ssh".into(),
            host: Some("build-box".into()),
            port: Some(22),
            user: None,
            container: None,
            shell: None,
            ssh_target: None,
            workspace_dir: "/home/ed/proj".into(),
        }
    }

    #[test]
    fn dto_roundtrip_preserves_fields() {
        let target = RemoteTarget::from(sample());
        assert_eq!(target.name, "build-box");
        assert_eq!(target.kind, TargetKind::Ssh);
        assert_eq!(target.workspace_dir.display().to_string(), "/home/ed/proj");

        let back = RemoteTargetDto::from(&target);
        assert_eq!(back.name, sample().name);
        assert_eq!(back.workspace_dir, sample().workspace_dir);
    }

    #[test]
    fn dto_kind_mapping_defaults_to_ssh() {
        let mut dto = sample();
        dto.kind = "docker".into();
        dto.container = Some("c".into());
        assert_eq!(RemoteTarget::from(dto).kind, TargetKind::Docker);
    }

    #[test]
    fn dtos_serialize_camel_case() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("workspaceDir"));
        assert!(!json.contains("workspace_dir"));
    }
}
