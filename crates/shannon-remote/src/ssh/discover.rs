//! `~/.ssh/config` host discovery.
//!
//! Parses the user's ssh config into candidate hosts so the connection UI can
//! offer "hosts the user already knows about". Shannon only ever references
//! the alias — sensitive fields (IdentityFile, ProxyCommand, ...) stay in the
//! ssh config and are never copied anywhere.
//!
//! `include` directives are not followed (a `tracing::warn!` notes the skip);
//! patterns containing `*` or `?` are skipped because they do not name a
//! concrete host.

use std::path::{Path, PathBuf};

/// One concrete `Host <alias>` block resolved from the ssh config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostCandidate {
    pub alias: String,
    pub user: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
}

/// Parse ssh config text into alias blocks (pure; no subprocess).
///
/// Only the fields Shannon surfaces in UI are extracted. Aliases from
/// wildcard patterns are skipped.
pub fn parse_ssh_config(text: &str) -> Vec<SshHostCandidate> {
    let mut out: Vec<SshHostCandidate> = Vec::new();
    let mut warned_include = false;
    // Indices into `out` of the aliases in the most recent `Host` block:
    // fields apply to every alias of the block.
    let mut block: Vec<usize> = Vec::new();

    for raw in text.lines() {
        let stripped = strip_comment(raw);
        // OpenSSH accepts `=` between keyword and value; normalize it away
        // so `Port = 22` and `Port=22` parse like `Port 22`.
        let normalized;
        let line: &str = if stripped.contains('=') {
            normalized = stripped.replace('=', " ");
            normalized.trim()
        } else {
            stripped.trim()
        };
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = split_kv(line) else {
            continue;
        };
        let key = key.to_ascii_lowercase();
        match key.as_str() {
            "host" => {
                block.clear();
                for alias in value.split_whitespace() {
                    if alias.contains('*') || alias.contains('?') {
                        continue;
                    }
                    let alias = alias.to_string();
                    if !out.iter().any(|c| c.alias == alias) {
                        out.push(SshHostCandidate {
                            alias,
                            user: None,
                            hostname: None,
                            port: None,
                        });
                        block.push(out.len() - 1);
                    }
                }
            }
            "user" | "hostname" | "port" => {
                for idx in &block {
                    let candidate = &mut out[*idx];
                    match key.as_str() {
                        "user" => candidate.user = Some(value.to_string()),
                        "hostname" => candidate.hostname = Some(value.to_string()),
                        _ => candidate.port = value.parse().ok(),
                    }
                }
            }
            "include" => {
                if !warned_include {
                    tracing::warn!("ssh config `include` directives are not followed for discovery");
                    warned_include = true;
                }
            }
            _ => {}
        }
    }
    out
}

/// Strip a `#` comment that is not inside quotes (ssh config rarely quotes;
/// a simple `#` cut matches OpenSSH's parser closely enough for discovery).
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Split `Key Value` at the first whitespace run.
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let key = parts.next()?;
    let value = parts.next()?.trim();
    Some((key, value))
}

/// Locate the primary ssh config file.
fn ssh_config_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".ssh").join("config");
    path.exists().then_some(path)
}

/// Enumerate candidate hosts from the user's ssh config.
///
/// Returns an empty vec when no config exists — never an error, so UIs can
/// treat discovery as best-effort.
pub fn discover_ssh_hosts() -> Vec<SshHostCandidate> {
    let Some(path) = ssh_config_path() else {
        return Vec::new();
    };
    read_candidates(&path)
}

fn read_candidates(path: &Path) -> Vec<SshHostCandidate> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_ssh_config(&text),
        Err(e) => {
            tracing::warn!("cannot read ssh config {}: {e}", path.display());
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
# Global settings
Host *
    ServerAliveInterval 60
    Compression yes

Host build-box
    HostName 192.168.1.20
    User ed
    Port 2222
    IdentityFile ~/.ssh/id_ed25519

Host gpu-1 gpu-2   # multiple aliases on one line
    User deploy

Host *.internal
    ProxyJump bastion

Host exact
    HostName = v2.example.com
    Port = 2233
"#;

    #[test]
    fn parses_aliases_and_skips_wildcards() {
        let hosts = parse_ssh_config(CONFIG);
        let names: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, vec!["build-box", "gpu-1", "gpu-2", "exact"]);
    }

    #[test]
    fn extracts_user_hostname_port() {
        let hosts = parse_ssh_config(CONFIG);
        let build = &hosts[0];
        assert_eq!(build.user.as_deref(), Some("ed"));
        assert_eq!(build.hostname.as_deref(), Some("192.168.1.20"));
        assert_eq!(build.port, Some(2222));
    }

    #[test]
    fn multiple_aliases_share_one_block() {
        let hosts = parse_ssh_config(CONFIG);
        assert_eq!(hosts[1].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[2].user.as_deref(), Some("deploy"));
    }

    #[test]
    fn equals_sign_form_and_comments_parse() {
        let hosts = parse_ssh_config(CONFIG);
        let exact = hosts.iter().find(|h| h.alias == "exact").unwrap();
        assert_eq!(exact.hostname.as_deref(), Some("v2.example.com"));
        assert_eq!(exact.port, Some(2233));
    }

    #[test]
    fn empty_and_garbage_input_are_tolerated() {
        assert!(parse_ssh_config("").is_empty());
        assert!(parse_ssh_config("this is not ssh config").is_empty());
        assert!(parse_ssh_config("# only a comment").is_empty());
    }

    #[test]
    fn trailing_field_after_wildcard_block_attaches_to_wildcard_skip() {
        // Fields under `Host *` must not leak into later real hosts.
        let hosts = parse_ssh_config("Host *\n  User nobody\nHost real\n  Port 2200\n");
        let real = hosts.iter().find(|h| h.alias == "real").unwrap();
        assert_eq!(real.user, None);
        assert_eq!(real.port, Some(2200));
    }

    #[test]
    fn missing_config_file_discovery_is_empty() {
        // read_candidates on a nonexistent path must not panic.
        assert!(read_candidates(Path::new("/nonexistent/ssh/config")).is_empty());
    }
}
