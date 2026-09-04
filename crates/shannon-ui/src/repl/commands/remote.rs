//! `/remote` — connect Shannon's tools to SSH hosts and Docker containers.
//!
//! Dashboard: `/remote` · switch: `/remote use <name>` · register:
//! `/remote add ssh|docker ...` · verify: `/remote test <name>` · leave:
//! `/remote disconnect`. Configuration persists in `~/.shannon/remotes.toml`
//! (see `shannon_remote::target`); credentials are never stored here —
//! authentication rides on the system ssh.

use shannon_remote::assembly::DynamicAssembly;
use shannon_remote::target::{RemoteTarget, RemotesFile, TargetKind};

use super::set_error;
use crate::Result;
use crate::repl::Repl;
use crate::widgets::ChatRole;
use rust_i18n::t;

/// Parsed `/remote` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoteAction {
    Dashboard,
    Use(String),
    Test(String),
    Disconnect,
    Reconnect,
    Remove {
        name: String,
        confirmed: bool,
    },
    AddSsh {
        destination: String,
        name: String,
        workspace_dir: String,
    },
    AddDocker {
        container: String,
        name: String,
        workspace_dir: String,
    },
    Unknown(String),
}

/// Pure parser (unit-tested): maps `/remote <args>` to an action.
pub(crate) fn parse_remote_args(args: &str) -> RemoteAction {
    let args = args.trim();
    let mut it = args.split_whitespace();
    let Some(sub) = it.next() else {
        return RemoteAction::Dashboard;
    };
    let rest = args[sub.len()..].trim();
    match sub {
        "list" | "" => RemoteAction::Dashboard,
        "use" => match rest.split_whitespace().next() {
            Some(name) => RemoteAction::Use(name.to_string()),
            None => RemoteAction::Unknown("use".into()),
        },
        "test" => match rest.split_whitespace().next() {
            Some(name) => RemoteAction::Test(name.to_string()),
            None => RemoteAction::Unknown("test".into()),
        },
        "disconnect" => RemoteAction::Disconnect,
        "reconnect" => RemoteAction::Reconnect,
        "remove" => {
            let mut parts = rest.split_whitespace();
            let Some(name) = parts.next() else {
                return RemoteAction::Unknown("remove".into());
            };
            RemoteAction::Remove {
                name: name.to_string(),
                confirmed: parts.next() == Some("--yes"),
            }
        }
        "add" => {
            let mut parts = rest.split_whitespace();
            match parts.next() {
                Some("ssh") => match (parts.next(), parts.next(), parts.next()) {
                    (Some(destination), Some(name), Some(workspace_dir)) => RemoteAction::AddSsh {
                        destination: destination.to_string(),
                        name: name.to_string(),
                        workspace_dir: workspace_dir.to_string(),
                    },
                    _ => RemoteAction::Unknown("add ssh".into()),
                },
                Some("docker") => match (parts.next(), parts.next(), parts.next()) {
                    (Some(container), Some(name), Some(workspace_dir)) => RemoteAction::AddDocker {
                        container: container.to_string(),
                        name: name.to_string(),
                        workspace_dir: workspace_dir.to_string(),
                    },
                    _ => RemoteAction::Unknown("add docker".into()),
                },
                _ => RemoteAction::Unknown("add".into()),
            }
        }
        other => RemoteAction::Unknown(other.to_string()),
    }
}

/// Entry point wired into the REPL command dispatcher.
pub(crate) fn handle_remote(repl: &mut Repl, args: &str) -> Result<()> {
    let Some(assembly) = repl.remote_assembly.clone() else {
        set_error(repl, "Remote worlds are unavailable in this session.");
        return Ok(());
    };
    match parse_remote_args(args) {
        RemoteAction::Dashboard => show_dashboard(repl, &assembly),
        RemoteAction::Use(name) => use_target(repl, &assembly, &name),
        RemoteAction::Test(name) => test_target(repl, &assembly, &name),
        RemoteAction::Disconnect => {
            shannon_remote::assembly::disconnect_dynamic(&assembly);
            repl.chat
                .add_message(ChatRole::System, t!("commands.remote.disconnected").into());
            Ok(())
        }
        RemoteAction::Reconnect => reconnect(repl, &assembly),
        RemoteAction::Remove { name, confirmed } => remove_target(repl, &name, confirmed),
        RemoteAction::AddSsh {
            destination,
            name,
            workspace_dir,
        } => add_target(repl, TargetKind::Ssh, destination, name, workspace_dir),
        RemoteAction::AddDocker {
            container,
            name,
            workspace_dir,
        } => add_target(repl, TargetKind::Docker, container, name, workspace_dir),
        RemoteAction::Unknown(sub) => {
            set_error(repl, t!("commands.remote.usage").as_ref());
            let _ = sub;
            Ok(())
        }
    }
}

fn show_dashboard(repl: &mut Repl, assembly: &DynamicAssembly) -> Result<()> {
    let mut lines = vec![t!("commands.remote.dashboard_title").into()];
    let file = RemotesFile::load_default();
    if file.targets.is_empty() {
        lines.push(t!("commands.remote.none").into());
    } else {
        let active = assembly.state.active_target();
        for target in &file.targets {
            let marker = if active.as_deref() == Some(target.name.as_str()) {
                "▸"
            } else {
                " "
            };
            let detail = match target.kind {
                TargetKind::Ssh => target.host.clone().unwrap_or_default(),
                TargetKind::Docker => target.container.clone().unwrap_or_default(),
            };
            lines.push(format!(
                "{marker} {name} ({kind}) → {detail} · {ws}",
                name = target.name,
                kind = target.kind,
                ws = target.workspace_dir.display()
            ));
        }
    }
    lines.push(t!("commands.remote.hint_ssh").into());
    lines.push(t!("commands.remote.usage").into());
    for line in lines {
        repl.chat.add_message(ChatRole::System, line);
    }
    Ok(())
}

fn use_target(repl: &mut Repl, assembly: &DynamicAssembly, name: &str) -> Result<()> {
    if assembly.state.active_target().as_deref() == Some(name)
        && assembly.state.status() == shannon_remote::ssh::WorldStatus::Connected
    {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.remote.already_connected", name = name).into(),
        );
        return Ok(());
    }
    let Some(target) = RemotesFile::load_default().resolve(name).cloned() else {
        set_error(repl, t!("commands.remote.not_found", name = name).as_ref());
        return Ok(());
    };
    let health = match repl
        .runtime
        .block_on(shannon_remote::assembly::connect_dynamic(assembly, &target))
    {
        Ok(h) => h,
        Err(e) => {
            set_error(
                repl,
                t!(
                    "commands.remote.test_failed",
                    name = name,
                    error = e.to_string()
                )
                .as_ref(),
            );
            return Ok(());
        }
    };
    // The agent now works on the remote workspace by default.
    repl.state.working_directory = target.workspace_dir.display().to_string();
    repl.chat.add_message(
        ChatRole::System,
        t!(
            "commands.remote.connected",
            name = name,
            platform = health.platform,
            latency = health.latency_ms
        )
        .to_string(),
    );
    if !health.bash_available {
        repl.chat
            .add_message(ChatRole::System, t!("commands.remote.bash_missing").into());
    }
    Ok(())
}

fn test_target(repl: &mut Repl, assembly: &DynamicAssembly, name: &str) -> Result<()> {
    let Some(target) = RemotesFile::load_default().resolve(name).cloned() else {
        set_error(repl, t!("commands.remote.not_found", name = name).as_ref());
        return Ok(());
    };
    // Health check via a throwaway connection (does not switch the world).
    let result = repl.runtime.block_on(async {
        match target.kind {
            TargetKind::Ssh => {
                let runtime = shannon_remote::ssh::SshRuntime::connect(&target).await?;
                runtime.health().await
            }
            TargetKind::Docker => Err(std::io::Error::other(
                "docker targets are validated on connect",
            )),
        }
    });
    let message = match result {
        Ok(h) => t!(
            "commands.remote.test_ok",
            name = name,
            platform = h.platform,
            latency = h.latency_ms,
            bash = if h.bash_available { "yes" } else { "no" },
            ws = if h.workspace_exists { "yes" } else { "no" }
        )
        .to_string(),
        Err(e) => t!(
            "commands.remote.test_failed",
            name = name,
            error = e.to_string()
        )
        .into(),
    };
    repl.chat.add_message(ChatRole::System, message);
    let _ = assembly; // dashboard state untouched by test
    Ok(())
}

fn reconnect(repl: &mut Repl, assembly: &DynamicAssembly) -> Result<()> {
    let Some(name) = assembly.state.active_target() else {
        set_error(repl, t!("commands.remote.not_connected").as_ref());
        return Ok(());
    };
    let target = RemotesFile::load_default().resolve(&name).cloned();
    match target {
        Some(target) => match repl
            .runtime
            .block_on(shannon_remote::assembly::connect_dynamic(assembly, &target))
        {
            Ok(_) => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.remote.reconnected", name = name).into(),
                );
            }
            Err(e) => {
                set_error(
                    repl,
                    t!("commands.remote.reconnect_failed", error = e.to_string()).as_ref(),
                );
            }
        },
        None => set_error(repl, t!("commands.remote.not_found", name = name).as_ref()),
    }
    Ok(())
}

fn remove_target(repl: &mut Repl, name: &str, confirmed: bool) -> Result<()> {
    let path = shannon_remote::target::remotes_path();
    let mut file = RemotesFile::load(&path).unwrap_or_default();
    if file.resolve(name).is_none() {
        set_error(repl, t!("commands.remote.not_found", name = name).as_ref());
        return Ok(());
    }
    if !confirmed {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.remote.remove_confirm", name = name).into(),
        );
        return Ok(());
    }
    file.remove(name);
    if let Err(e) = file.save(&path) {
        set_error(repl, &e.to_string());
        return Ok(());
    }
    repl.chat.add_message(
        ChatRole::System,
        t!("commands.remote.remove_done", name = name).into(),
    );
    Ok(())
}

fn add_target(
    repl: &mut Repl,
    kind: TargetKind,
    detail: String,
    name: String,
    workspace_dir: String,
) -> Result<()> {
    let workspace_dir = std::path::PathBuf::from(workspace_dir);
    let target = match kind {
        TargetKind::Ssh => RemoteTarget {
            name,
            kind,
            host: Some(detail),
            port: None,
            user: None,
            container: None,
            shell: None,
            ssh_target: None,
            workspace_dir,
        },
        TargetKind::Docker => RemoteTarget {
            name,
            kind,
            host: None,
            port: None,
            user: None,
            container: Some(detail),
            shell: None,
            ssh_target: None,
            workspace_dir,
        },
    };
    let name = target.name.clone();
    if let Err(e) = target.validate() {
        set_error(repl, &e.to_string());
        return Ok(());
    }
    let path = shannon_remote::target::remotes_path();
    let mut file = RemotesFile::load(&path).unwrap_or_default();
    if let Err(e) = file.upsert(target) {
        set_error(repl, &e.to_string());
        return Ok(());
    }
    if let Err(e) = file.save(&path) {
        set_error(repl, &e.to_string());
        return Ok(());
    }
    repl.chat.add_message(
        ChatRole::System,
        t!(
            "commands.remote.added",
            name = name,
            kind = kind.to_string()
        )
        .into(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dashboard_and_empty_args() {
        assert_eq!(parse_remote_args(""), RemoteAction::Dashboard);
        assert_eq!(parse_remote_args("  "), RemoteAction::Dashboard);
        assert_eq!(parse_remote_args("list"), RemoteAction::Dashboard);
    }

    #[test]
    fn parses_use_test_disconnect_reconnect() {
        assert_eq!(
            parse_remote_args("use build-box"),
            RemoteAction::Use("build-box".into())
        );
        assert_eq!(
            parse_remote_args("use   "),
            RemoteAction::Unknown("use".into())
        );
        assert_eq!(
            parse_remote_args("test ci"),
            RemoteAction::Test("ci".into())
        );
        assert_eq!(parse_remote_args("disconnect"), RemoteAction::Disconnect);
        assert_eq!(parse_remote_args("reconnect"), RemoteAction::Reconnect);
    }

    #[test]
    fn parses_remove_with_optional_confirmation() {
        assert_eq!(
            parse_remote_args("remove build-box"),
            RemoteAction::Remove {
                name: "build-box".into(),
                confirmed: false
            }
        );
        assert_eq!(
            parse_remote_args("remove build-box --yes"),
            RemoteAction::Remove {
                name: "build-box".into(),
                confirmed: true
            }
        );
        assert_eq!(
            parse_remote_args("remove"),
            RemoteAction::Unknown("remove".into())
        );
    }

    #[test]
    fn parses_add_ssh_and_add_docker() {
        assert_eq!(
            parse_remote_args("add ssh ed@build-box build /home/ed/proj"),
            RemoteAction::AddSsh {
                destination: "ed@build-box".into(),
                name: "build".into(),
                workspace_dir: "/home/ed/proj".into(),
            }
        );
        assert_eq!(
            parse_remote_args("add docker shannon-ci ci /workspace"),
            RemoteAction::AddDocker {
                container: "shannon-ci".into(),
                name: "ci".into(),
                workspace_dir: "/workspace".into(),
            }
        );
        // Missing workspace_dir → usage error
        assert_eq!(
            parse_remote_args("add ssh ed@host build"),
            RemoteAction::Unknown("add ssh".into())
        );
        assert_eq!(
            parse_remote_args("add k8s foo"),
            RemoteAction::Unknown("add".into())
        );
    }

    #[test]
    fn unknown_subcommand_surfaces_usage() {
        assert_eq!(
            parse_remote_args("wat"),
            RemoteAction::Unknown("wat".into())
        );
    }
}

// ── Handler tests (Repl::new() runs in minimal init under cfg(test)) ────

#[cfg(test)]
mod handler_tests {
    use super::*;
    use crate::repl::Repl;
    use shannon_remote::assembly::assemble_dynamic;
    use shannon_remote::target::RemotesFile;

    /// Point HOME at a scratch dir so remotes.toml never touches the real
    /// one. nextest runs each test in its own process, so the env swap is
    /// process-local and race-free.
    struct HomeGuard(std::path::PathBuf);
    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            // SAFETY: single-threaded test process (nextest isolation); no
            // other thread reads HOME during this test.
            unsafe { std::env::set_var("HOME", dir.path()) };
            Self(dir.path().to_path_buf())
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: see new()
            unsafe { std::env::set_var("HOME", "/") };
        }
    }

    fn repl_with_world() -> Repl {
        let mut repl = Repl::new().expect("minimal repl");
        repl.remote_assembly = Some(std::sync::Arc::new(assemble_dynamic()));
        repl
    }

    fn last_message(repl: &Repl) -> String {
        repl.chat
            .messages()
            .back()
            .map(|m| m.content.clone())
            .unwrap_or_default()
    }

    #[test]
    fn add_ssh_target_persists_and_dashboard_lists_it() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();

        handle_remote(&mut repl, "add ssh ed@build-box build /home/ed/proj").unwrap();
        assert!(last_message(&repl).contains("build"), "add confirmation");

        let file = RemotesFile::load_default();
        let saved = file.resolve("build").expect("target persisted");
        assert_eq!(saved.ssh_destination(), "ed@build-box");
        assert_eq!(saved.workspace_dir.display().to_string(), "/home/ed/proj");

        // Dashboard renders the saved target.
        handle_remote(&mut repl, "").unwrap();
        let all: String = repl
            .chat
            .messages()
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("build (ssh)"), "dashboard lists target: {all}");
    }

    #[test]
    fn add_docker_target_validates_workspace() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();

        // Relative workspace_dir must be rejected via the validate path.
        handle_remote(&mut repl, "add docker ci ci-run relative/path").unwrap();
        assert!(
            last_message(&repl).starts_with("Error:"),
            "relative workspace rejected: {}",
            last_message(&repl)
        );

        // Absolute path persists (arg order: <container> <name> <workspace>).
        handle_remote(&mut repl, "add docker ci ci-run /workspace").unwrap();
        let file = RemotesFile::load_default();
        let saved = file.resolve("ci-run").expect("docker target persisted");
        assert_eq!(saved.container.as_deref(), Some("ci"));
    }

    #[test]
    fn remove_requires_confirmation_then_deletes() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();
        handle_remote(&mut repl, "add ssh host-x x /x").unwrap();

        // First call asks for confirmation and keeps the target.
        handle_remote(&mut repl, "remove x").unwrap();
        assert!(RemotesFile::load_default().resolve("x").is_some());
        assert!(last_message(&repl).contains("/remote remove x --yes"));

        // --yes deletes it.
        handle_remote(&mut repl, "remove x --yes").unwrap();
        assert!(RemotesFile::load_default().resolve("x").is_none());
    }

    #[test]
    fn use_unknown_target_reports_error_and_stays_local() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();
        handle_remote(&mut repl, "use nope").unwrap();
        assert!(last_message(&repl).starts_with("Error:"));
        let assembly = repl.remote_assembly.as_ref().unwrap();
        assert!(!assembly.world.is_remote(), "local world stays installed");
    }

    #[test]
    fn disconnect_restores_local_status() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();
        handle_remote(&mut repl, "disconnect").unwrap();
        assert!(
            last_message(&repl).contains("local"),
            "msg: {}",
            last_message(&repl)
        );
        let assembly = repl.remote_assembly.as_ref().unwrap();
        assert_eq!(
            assembly.state.status(),
            shannon_remote::ssh::WorldStatus::Local
        );
    }

    #[test]
    fn test_unknown_target_errors_without_connecting() {
        let _home = HomeGuard::new();
        let mut repl = repl_with_world();
        handle_remote(&mut repl, "test ghost").unwrap();
        assert!(last_message(&repl).starts_with("Error:"));
    }
}
