//! P5 Native data source catalog.
//!
//! Tier-1 native Rust integrations — no upstream fetch, no installer download.
//! Each entry is a catalog row that, when installed, prompts the user for
//! adapter-specific config (vault path, IMAP credentials) and writes the
//! config to `~/.shannon/data-sources/<slug>.toml`.
//!
//! Six adapters ship today:
//! 1. **Obsidian Vault** — reads markdown notes from a local vault directory.
//! 2. **Email (IMAP)** — connects to an IMAP server to read mailbox messages.
//! 3. **Notion** — queries pages/databases via the Notion REST API.
//! 4. **Linear** — queries issues via the Linear GraphQL API.
//! 5. **GitHub Issues** — queries issues/PRs via the GitHub REST API.
//! 6. **Jira** — queries issues via the Jira Cloud REST API.

use serde::{Deserialize, Serialize};

use super::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};

/// Adapter identifier embedded in catalog metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSourceKind {
    Obsidian,
    EmailImap,
    Notion,
    Linear,
    #[serde(rename = "github_issues")]
    GitHubIssues,
    Jira,
}

impl DataSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataSourceKind::Obsidian => "obsidian",
            DataSourceKind::EmailImap => "email_imap",
            DataSourceKind::Notion => "notion",
            DataSourceKind::Linear => "linear",
            DataSourceKind::GitHubIssues => "github_issues",
            DataSourceKind::Jira => "jira",
        }
    }
}

/// Field descriptor — drives the install form on the UI side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceField {
    /// Form input key, e.g. `vault_path`.
    pub key: String,
    /// Human label, e.g. `Vault path`.
    pub label: String,
    /// `text` | `password` | `path` | `number`.
    #[serde(default = "default_field_kind")]
    pub kind: String,
    /// Whether the field must be non-empty before install proceeds.
    #[serde(default = "default_required")]
    pub required: bool,
    /// Placeholder shown in the UI.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Help text shown below the field.
    #[serde(default)]
    pub help: Option<String>,
}

fn default_field_kind() -> String {
    "text".into()
}

fn default_required() -> bool {
    true
}

/// Static description of a native data source adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceAdapter {
    pub slug: String,
    pub kind: DataSourceKind,
    pub name: String,
    pub description: String,
    pub homepage_url: Option<String>,
    pub fields: Vec<DataSourceField>,
}

/// Static catalog of native data source adapters.
pub fn data_source_adapters() -> Vec<DataSourceAdapter> {
    vec![
        DataSourceAdapter {
            slug: "obsidian-vault".into(),
            kind: DataSourceKind::Obsidian,
            name: "Obsidian Vault".into(),
            description: "Read markdown notes from a local Obsidian vault.".into(),
            homepage_url: Some("https://obsidian.md".into()),
            fields: vec![
                DataSourceField {
                    key: "vault_path".into(),
                    label: "Vault path".into(),
                    kind: "path".into(),
                    required: true,
                    placeholder: Some("/home/user/MyVault".into()),
                    help: Some("Absolute path to the vault root.".into()),
                },
                DataSourceField {
                    key: "include_attachments".into(),
                    label: "Index attachments".into(),
                    kind: "text".into(),
                    required: false,
                    placeholder: Some("false".into()),
                    help: Some("Whether to index PDF/image attachments alongside notes.".into()),
                },
            ],
        },
        DataSourceAdapter {
            slug: "email-imap".into(),
            kind: DataSourceKind::EmailImap,
            name: "Email (IMAP)".into(),
            description: "Connect to an IMAP server to read mailbox messages.".into(),
            homepage_url: None,
            fields: vec![
                DataSourceField {
                    key: "imap_host".into(),
                    label: "IMAP host".into(),
                    kind: "text".into(),
                    required: true,
                    placeholder: Some("imap.gmail.com".into()),
                    help: None,
                },
                DataSourceField {
                    key: "imap_port".into(),
                    label: "IMAP port".into(),
                    kind: "number".into(),
                    required: true,
                    placeholder: Some("993".into()),
                    help: None,
                },
                DataSourceField {
                    key: "username".into(),
                    label: "Username".into(),
                    kind: "text".into(),
                    required: true,
                    placeholder: Some("you@example.com".into()),
                    help: None,
                },
                DataSourceField {
                    key: "password".into(),
                    label: "Password / app password".into(),
                    kind: "password".into(),
                    required: true,
                    placeholder: None,
                    help: Some("Use an app-specific password when 2FA is enabled.".into()),
                },
            ],
        },
        DataSourceAdapter {
            slug: "notion".into(),
            kind: DataSourceKind::Notion,
            name: "Notion".into(),
            description: "Query Notion pages and databases via the REST API.".into(),
            homepage_url: Some("https://developers.notion.com/".into()),
            fields: vec![
                DataSourceField {
                    key: "integration_token".into(),
                    label: "Integration token".into(),
                    kind: "password".into(),
                    required: true,
                    placeholder: Some("secret_...".into()),
                    help: Some(
                        "Create an internal integration at notion.so/my-integrations.".into(),
                    ),
                },
                DataSourceField {
                    key: "database_id".into(),
                    label: "Default database ID".into(),
                    kind: "text".into(),
                    required: false,
                    placeholder: Some("32-char hex ID".into()),
                    help: Some("Optional: pre-filter searches to this database.".into()),
                },
            ],
        },
        DataSourceAdapter {
            slug: "linear".into(),
            kind: DataSourceKind::Linear,
            name: "Linear".into(),
            description: "Query Linear issues via the GraphQL API.".into(),
            homepage_url: Some("https://developers.linear.app/".into()),
            fields: vec![
                DataSourceField {
                    key: "api_key".into(),
                    label: "Personal API key".into(),
                    kind: "password".into(),
                    required: true,
                    placeholder: Some("lin_api_...".into()),
                    help: Some("Generate at linear.app/settings/api".into()),
                },
                DataSourceField {
                    key: "team_key".into(),
                    label: "Default team key".into(),
                    kind: "text".into(),
                    required: false,
                    placeholder: Some("ENG".into()),
                    help: Some("Optional: pre-filter queries to one team.".into()),
                },
            ],
        },
        DataSourceAdapter {
            slug: "github-issues".into(),
            kind: DataSourceKind::GitHubIssues,
            name: "GitHub Issues".into(),
            description: "Query issues and pull requests via the GitHub REST API.".into(),
            homepage_url: Some("https://docs.github.com/rest".into()),
            fields: vec![
                DataSourceField {
                    key: "token".into(),
                    label: "Personal access token".into(),
                    kind: "password".into(),
                    required: true,
                    placeholder: Some("ghp_...".into()),
                    help: Some("Needs `repo` (classic) or `issues:read` (fine-grained).".into()),
                },
                DataSourceField {
                    key: "default_repo".into(),
                    label: "Default repo (owner/name)".into(),
                    kind: "text".into(),
                    required: false,
                    placeholder: Some("shannon-agent/shannon-code".into()),
                    help: Some("Optional: pre-filter queries to one repository.".into()),
                },
            ],
        },
        DataSourceAdapter {
            slug: "jira".into(),
            kind: DataSourceKind::Jira,
            name: "Jira".into(),
            description: "Query Jira issues via the Cloud REST API.".into(),
            homepage_url: Some(
                "https://developer.atlassian.com/cloud/jira/platform/rest/v3/".into(),
            ),
            fields: vec![
                DataSourceField {
                    key: "domain".into(),
                    label: "Jira Cloud domain".into(),
                    kind: "text".into(),
                    required: true,
                    placeholder: Some("your-team.atlassian.net".into()),
                    help: None,
                },
                DataSourceField {
                    key: "email".into(),
                    label: "Account email".into(),
                    kind: "text".into(),
                    required: true,
                    placeholder: Some("you@team.com".into()),
                    help: None,
                },
                DataSourceField {
                    key: "api_token".into(),
                    label: "API token".into(),
                    kind: "password".into(),
                    required: true,
                    placeholder: None,
                    help: Some(
                        "Create at id.atlassian.com/manage-profile/security/api-tokens.".into(),
                    ),
                },
                DataSourceField {
                    key: "project_key".into(),
                    label: "Default project key".into(),
                    kind: "text".into(),
                    required: false,
                    placeholder: Some("SHAN".into()),
                    help: Some("Optional: pre-filter queries to one project.".into()),
                },
            ],
        },
    ]
}

/// Convert the static adapter list into catalog entries for the unified hub.
pub fn data_source_catalog_entries() -> Vec<CatalogEntry> {
    data_source_adapters()
        .into_iter()
        .map(|adapter| adapter_to_entry(&adapter))
        .collect()
}

fn adapter_to_entry(adapter: &DataSourceAdapter) -> CatalogEntry {
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("kind".to_string(), serde_json::json!(adapter.kind.as_str()));
    metadata.insert("fields".to_string(), serde_json::json!(adapter.fields));
    CatalogEntry {
        id: format!("native:data-source-{}", adapter.slug),
        kind: AddonKind::DataSource,
        name: adapter.name.clone(),
        description: adapter.description.clone(),
        author: Some("Shannon".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        homepage_url: adapter.homepage_url.clone(),
        license: Some("Apache-2.0".into()),
        stars: None,
        last_updated: None,
        source: CatalogSource::Native,
        trust: TrustLevel::Verified,
        metadata,
        tags: vec!["native".into(), adapter.kind.as_str().into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_include_obsidian_and_email() {
        let adapters = data_source_adapters();
        let slugs: Vec<&str> = adapters.iter().map(|a| a.slug.as_str()).collect();
        assert!(slugs.contains(&"obsidian-vault"));
        assert!(slugs.contains(&"email-imap"));
        assert!(slugs.contains(&"notion"));
        assert!(slugs.contains(&"linear"));
        assert!(slugs.contains(&"github-issues"));
        assert!(slugs.contains(&"jira"));
    }

    #[test]
    fn catalog_entries_have_native_source_and_verified_trust() {
        let entries = data_source_catalog_entries();
        assert_eq!(entries.len(), 6);
        for entry in &entries {
            assert_eq!(entry.kind, AddonKind::DataSource);
            assert_eq!(entry.source, CatalogSource::Native);
            assert_eq!(entry.trust, TrustLevel::Verified);
            assert!(entry.id.starts_with("native:data-source-"));
        }
    }

    #[test]
    fn obsidian_entry_requires_vault_path() {
        let entries = data_source_catalog_entries();
        let obsidian = entries
            .iter()
            .find(|e| e.name == "Obsidian Vault")
            .expect("obsidian");
        let fields = obsidian.metadata.get("fields").expect("fields");
        let fields: Vec<DataSourceField> =
            serde_json::from_value(fields.clone()).expect("deserialize fields");
        let vault_path = fields
            .iter()
            .find(|f| f.key == "vault_path")
            .expect("vault_path field");
        assert!(vault_path.required);
        assert_eq!(vault_path.kind, "path");
    }

    #[test]
    fn email_entry_has_password_field() {
        let entries = data_source_catalog_entries();
        let email = entries
            .iter()
            .find(|e| e.name == "Email (IMAP)")
            .expect("email");
        let fields = email.metadata.get("fields").expect("fields");
        let fields: Vec<DataSourceField> =
            serde_json::from_value(fields.clone()).expect("deserialize fields");
        let password = fields
            .iter()
            .find(|f| f.key == "password")
            .expect("password field");
        assert_eq!(password.kind, "password");
        assert!(password.required);
    }

    #[test]
    fn kind_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&DataSourceKind::EmailImap).unwrap(),
            "\"email_imap\""
        );
        assert_eq!(
            serde_json::to_string(&DataSourceKind::Obsidian).unwrap(),
            "\"obsidian\""
        );
        assert_eq!(
            serde_json::to_string(&DataSourceKind::GitHubIssues).unwrap(),
            "\"github_issues\""
        );
    }

    #[test]
    fn notion_adapter_has_integration_token_field() {
        let entries = data_source_catalog_entries();
        let notion = entries.iter().find(|e| e.name == "Notion").expect("notion");
        let fields = notion.metadata.get("fields").expect("fields");
        let fields: Vec<DataSourceField> =
            serde_json::from_value(fields.clone()).expect("deserialize fields");
        let token = fields
            .iter()
            .find(|f| f.key == "integration_token")
            .expect("integration_token field");
        assert_eq!(token.kind, "password");
        assert!(token.required);
    }

    #[test]
    fn jira_adapter_requires_domain_email_and_token() {
        let entries = data_source_catalog_entries();
        let jira = entries.iter().find(|e| e.name == "Jira").expect("jira");
        let fields = jira.metadata.get("fields").expect("fields");
        let fields: Vec<DataSourceField> =
            serde_json::from_value(fields.clone()).expect("deserialize fields");
        let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
        assert!(keys.contains(&"domain"));
        assert!(keys.contains(&"email"));
        assert!(keys.contains(&"api_token"));
        let token = fields.iter().find(|f| f.key == "api_token").expect("token");
        assert_eq!(token.kind, "password");
    }
}
