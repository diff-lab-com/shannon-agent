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
        let output = format_credentials_list();
        assert!(output.contains("Stored Credentials"));
        assert!(output.contains("/credentials list"));
    }

    #[test]
    fn test_format_credential_count() {
        let output = format_credential_count();
        assert!(output.contains("Stored credentials"));
    }
}
