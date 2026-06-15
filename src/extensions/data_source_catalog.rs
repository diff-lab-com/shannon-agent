//! P5 Native data source catalog.
//!
//! Tier-1 native Rust integrations — no upstream fetch, no installer download.
//! Each entry is a catalog row that, when installed, prompts the user for
//! adapter-specific config (vault path, IMAP credentials) and writes the
//! config to `~/.shannon/data-sources/<slug>.toml`.
//!
//! Two adapters ship today:
//! 1. **Obsidian Vault** — reads markdown notes from a local vault directory.
//! 2. **Email (IMAP)** — connects to an IMAP server to read mailbox messages.

use serde::{Deserialize, Serialize};

use super::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};

/// Adapter identifier embedded in catalog metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSourceKind {
    Obsidian,
    EmailImap,
}

impl DataSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataSourceKind::Obsidian => "obsidian",
            DataSourceKind::EmailImap => "email_imap",
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
                    help: Some(
                        "Whether to index PDF/image attachments alongside notes.".into(),
                    ),
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
                    help: Some(
                        "Use an app-specific password when 2FA is enabled.".into(),
                    ),
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
    metadata.insert(
        "kind".to_string(),
        serde_json::json!(adapter.kind.as_str()),
    );
    metadata.insert(
        "fields".to_string(),
        serde_json::json!(adapter.fields),
    );
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
    }

    #[test]
    fn catalog_entries_have_native_source_and_verified_trust() {
        let entries = data_source_catalog_entries();
        assert_eq!(entries.len(), 2);
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
    }
}
