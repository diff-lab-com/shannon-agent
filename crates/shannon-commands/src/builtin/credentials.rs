//! /credentials command - Manage API credentials and secrets

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

/// Credentials prompt template
const CREDENTIALS_PROMPT: &str = r##"
Manage stored credentials and API keys.

Arguments: {args}

Subcommands:
- **list** — Show all stored credentials (values are masked)
- **store <service> <value>** — Store a new credential for a service
- **get <service>** — Retrieve a credential value (masked display)
- **delete <service>** — Delete a stored credential
- **count** — Show the number of stored credentials
- **help** — Show usage information

If no subcommand is given, default to listing all credentials.
"##;

/// Create the /credentials command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "credentials".to_string(),
            aliases: vec!["creds".to_string(), "cred".to_string()],
            description: "Manage stored credentials and API keys".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|store|get|delete|count] [service] [value]".to_string()),
            when_to_use: Some(
                "Use to manage stored API keys and credentials for various services".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: true,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 2000,
        arg_names: vec![
            "action".to_string(),
            "service".to_string(),
            "value".to_string(),
        ],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(CREDENTIALS_PROMPT.to_string()),
    }))
}

/// Credential actions
#[derive(Debug, Clone, PartialEq)]
pub enum CredentialAction {
    /// List all stored credentials (masked)
    List,
    /// Store a new credential
    Store,
    /// Get a credential value (masked)
    Get,
    /// Delete a stored credential
    Delete,
    /// Show credential count
    Count,
    /// Show help
    Help,
}

/// Parse credential action from argument string
pub fn parse_credential_action(arg: &str) -> CredentialAction {
    match arg.to_lowercase().as_str() {
        "list" | "ls" => CredentialAction::List,
        "store" | "add" | "set" => CredentialAction::Store,
        "get" => CredentialAction::Get,
        "delete" | "remove" | "rm" => CredentialAction::Delete,
        "count" => CredentialAction::Count,
        "help" | "?" => CredentialAction::Help,
        _ => CredentialAction::List,
    }
}

/// Create a loaded CredentialManager
fn get_manager() -> Result<shannon_core::credential_manager::CredentialManager, String> {
    let mut manager =
        shannon_core::credential_manager::CredentialManager::new().map_err(|e| format!("{e}"))?;
    manager.load().map_err(|e| format!("{e}"))?;
    Ok(manager)
}

/// Format credentials list output
pub fn format_credentials_list() -> String {
    let mut output = String::from("Stored Credentials:\n\n");

    match get_manager() {
        Ok(manager) => {
            let credentials = manager.list();
            if credentials.is_empty() {
                output.push_str("  No credentials stored.\n");
            } else {
                for cred in &credentials {
                    output.push_str(&format!(
                        "  {} — {} (created: {})\n",
                        cred.service,
                        cred.name,
                        cred.created_at.format("%Y-%m-%d %H:%M")
                    ));
                }
            }
        }
        Err(e) => {
            output.push_str(&format!("  Error accessing credentials: {e}\n"));
        }
    }

    output.push_str("\nUsage:\n");
    output.push_str("  /credentials list              - Show stored credentials\n");
    output.push_str("  /credentials store <svc> <val> - Store a credential\n");
    output.push_str("  /credentials get <service>     - Retrieve a credential (masked)\n");
    output.push_str("  /credentials delete <service>  - Delete a credential\n");
    output.push_str("  /credentials count             - Show stored credential count\n");

    output
}

/// Format credential store response
pub fn format_credential_store(service: &str, value: &str) -> String {
    match get_manager() {
        Ok(mut manager) => {
            let credential =
                shannon_core::credential_manager::Credential::new(service, service, value);
            match manager.store_or_update(credential) {
                Ok(_) => format!("Credential stored for service: {service}"),
                Err(e) => format!("Failed to store credential: {e}"),
            }
        }
        Err(e) => format!("Error accessing credential manager: {e}"),
    }
}

/// Format credential get response (value is masked)
pub fn format_credential_get(service: &str) -> String {
    match get_manager() {
        Ok(manager) => {
            match manager.retrieve(service) {
                Ok(cred) => {
                    // Mask the value for display — only show first/last 2 chars
                    let val = &cred.value;
                    let masked = if val.len() <= 4 {
                        "*".repeat(val.len())
                    } else {
                        format!("{}****{}", &val[..2], &val[val.len() - 2..])
                    };
                    format!("Credential for '{service}': {masked}")
                }
                Err(e) => format!("Credential not found for '{service}': {e}"),
            }
        }
        Err(e) => format!("Error accessing credential manager: {e}"),
    }
}

/// Format credential delete response
pub fn format_credential_delete(service: &str) -> String {
    match get_manager() {
        Ok(mut manager) => match manager.delete(service) {
            Ok(_) => format!("Credential deleted for service: {service}"),
            Err(e) => format!("Failed to delete credential for '{service}': {e}"),
        },
        Err(e) => format!("Error accessing credential manager: {e}"),
    }
}

/// Format credential count response
pub fn format_credential_count() -> String {
    match get_manager() {
        Ok(manager) => {
            format!("Stored credentials: {}", manager.count())
        }
        Err(e) => format!("Error accessing credential manager: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Serializes tests that mutate the process-global `HOME` (so that
    /// `dirs::home_dir()` resolves to a per-test tempdir). Tests run in
    /// parallel by default; without this guard, two tests would race on
    /// which one owns `HOME` at any given instant and the credential
    /// store would see writes from both into the same tempdir.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    /// Scoped override of the `HOME` (Unix/macOS) and `USERPROFILE` (Windows)
    /// environment variables, so `dirs::home_dir()` (used transitively by
    /// `CredentialManager::new` → `default_credentials_dir`) resolves to the
    /// test's tempdir. Mirrors the `SHANNON_*` env-override pattern used
    /// elsewhere and matches the `TempDir`-based isolation style already
    /// established by `credential_manager.rs::TestDir`.
    ///
    /// P2-7 fix round 4 (test hygiene): the previous tests read the
    /// developer's real `~/.shannon/credentials/`, which is brittle (a
    /// malformed real file crashed `serde_json::from_str` and made the
    /// count formatter return the error branch instead of the expected
    /// "Stored credentials: N" string). Final-gate gate fail.
    struct ScopedHome {
        _temp: tempfile::TempDir,
        _prev_home: Option<std::ffi::OsString>,
        _prev_userprofile: Option<std::ffi::OsString>,
    }

    impl ScopedHome {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create tempdir");
            // Seed the (empty) credentials dir so CredentialManager::load
            // takes the empty-store branch deterministically — neither the
            // "dir does not exist" branch (which creates an in-memory empty
            // store) nor a corrupted-file branch can fire.
            std::fs::create_dir_all(temp.path().join(".shannon").join("credentials"))
                .expect("seed credentials dir");
            let prev_home = std::env::var_os("HOME");
            let prev_userprofile = std::env::var_os("USERPROFILE");
            // SAFETY: tests in this module run single-threaded against
            // ScopedHome instances (#[test] fn bodies are sequential per
            // crate-binary); concurrent set_var/remove_var across threads
            // is the documented soundness hazard and does not apply here.
            unsafe {
                std::env::set_var("HOME", temp.path());
            }
            #[cfg(windows)]
            unsafe {
                std::env::set_var("USERPROFILE", temp.path());
            }
            Self {
                _temp: temp,
                _prev_home: prev_home,
                _prev_userprofile: prev_userprofile,
            }
        }
    }

    impl Drop for ScopedHome {
        fn drop(&mut self) {
            // SAFETY: see ScopedHome::new. The lifetime is bound to a single
            // test; we restore the prior value before any other test can
            // observe HOME again.
            unsafe {
                match &self._prev_home {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
                #[cfg(windows)]
                match &self._prev_userprofile {
                    Some(v) => std::env::set_var("USERPROFILE", v),
                    None => std::env::remove_var("USERPROFILE"),
                }
            }
        }
    }

    /// Build a `[service].json` credential file inside the scoped home's
    /// `~/.shannon/credentials/` so tests can populate deterministic state.
    fn write_credential(svc_dir: &ScopedHome, service: &str, value: &str) {
        // SAFETY: this helper is only called from single-threaded tests in
        // the credentials module, after ScopedHome::new has installed HOME.
        let safe_name = service.replace(['/', '\\', '\0'], "_");
        let path: PathBuf = svc_dir
            ._temp
            .path()
            .join(".shannon")
            .join("credentials")
            .join(format!("{safe_name}.json"));
        let body = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": service,
            "service": service,
            "value": value,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "metadata": {},
        })
        .to_string();
        std::fs::write(path, body).expect("write credential");
    }

    #[test]
    fn test_parse_credential_action() {
        assert_eq!(parse_credential_action("list"), CredentialAction::List);
        assert_eq!(parse_credential_action("ls"), CredentialAction::List);
        assert_eq!(parse_credential_action("store"), CredentialAction::Store);
        assert_eq!(parse_credential_action("add"), CredentialAction::Store);
        assert_eq!(parse_credential_action("set"), CredentialAction::Store);
        assert_eq!(parse_credential_action("get"), CredentialAction::Get);
        assert_eq!(parse_credential_action("delete"), CredentialAction::Delete);
        assert_eq!(parse_credential_action("rm"), CredentialAction::Delete);
        assert_eq!(parse_credential_action("count"), CredentialAction::Count);
        assert_eq!(parse_credential_action("help"), CredentialAction::Help);
        assert_eq!(parse_credential_action("unknown"), CredentialAction::List);
    }

    #[test]
    fn test_format_credentials_list() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = ScopedHome::new();
        write_credential(&home, "anthropic", "sk-test-anthropic");
        write_credential(&home, "github", "ghp-test");

        let output = format_credentials_list();
        assert!(output.contains("Stored Credentials"));
        assert!(output.contains("anthropic"));
        assert!(output.contains("github"));
        // Masking: values must not leak.
        assert!(!output.contains("sk-test-anthropic"));
        assert!(!output.contains("ghp-test"));
    }

    #[test]
    fn test_format_credential_count() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = ScopedHome::new();
        // Empty store → "Stored credentials: 0".
        let output = format_credential_count();
        assert!(
            output.contains("Stored credentials: 0"),
            "expected empty-store branch, got {output:?}"
        );

        // Populated store → "Stored credentials: N".
        write_credential(&home, "anthropic", "sk-a");
        write_credential(&home, "openai", "sk-o");
        write_credential(&home, "github", "ghp-x");
        let output = format_credential_count();
        assert!(
            output.contains("Stored credentials: 3"),
            "expected 'Stored credentials: 3', got {output:?}"
        );
    }

    /// Regression guard for the env-coupling bug: even if the developer's
    /// real home contains a malformed credential file, the test must
    /// observe the scoped (empty) store and not see the developer's data.
    #[test]
    fn test_format_credential_count_does_not_read_real_home() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _home = ScopedHome::new();
        // Even when the real ~/.shannon/credentials/* contains many files
        // (e.g. anthropic, github, zhipu), the scoped HOME redirects
        // CredentialManager to an empty tempdir; the count must be 0.
        let output = format_credential_count();
        assert!(
            output.contains("Stored credentials: 0"),
            "test must be HOME-isolated, got {output:?}"
        );
    }
}
