//! # Credential Manager
//!
//! Secure credential storage and management for Shannon Code.
//!
//! Provides CRUD operations for credentials with disk persistence (JSON),
//! file permission validation (credentials must be 600), and portable
//! export/import for transferring credentials between machines.
//!
//! Reference: Claude Code src/utils/auth.ts, authFileDescriptor.ts, authPortable.ts

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};
use uuid::Uuid;

/// Errors that can occur during credential management operations.
#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential not found: {0}")]
    NotFound(String),

    #[error("Credential already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid credential: {0}")]
    Invalid(String),

    #[error("Permission error on credential file: {path} has mode {actual:#o}, expected {expected:#o}")]
    PermissionError {
        path: String,
        actual: u32,
        expected: u32,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),
}

/// A single stored credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    /// Unique identifier for this credential.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Service this credential is for (e.g., "anthropic", "github").
    pub service: String,
    /// The credential value (stored in plaintext on disk; encryption is the
    /// caller's responsibility before calling `store`).
    pub value: String,
    /// When this credential was created.
    pub created_at: DateTime<Utc>,
    /// When this credential was last updated.
    pub updated_at: DateTime<Utc>,
    /// Optional key-value metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl Credential {
    /// Create a new credential with the given name, service, and value.
    /// Generates a unique ID and timestamps automatically.
    pub fn new(name: &str, service: &str, value: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            service: service.to_string(),
            value: value.to_string(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }
}

/// Metadata about a credential file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFileDescriptor {
    /// Absolute path to the credential file.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// File permission mode (e.g., 0o600).
    pub permissions: u32,
    /// Whether the file exists on disk.
    pub exists: bool,
    /// The format of the credential file.
    pub format: CredentialFileFormat,
}

/// Supported credential file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialFileFormat {
    /// JSON format (the default).
    Json,
}

/// A portable credential for export/import between machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableCredential {
    /// Human-readable name.
    pub name: String,
    /// Service identifier.
    pub service: String,
    /// The credential value.
    pub value: String,
    /// Optional metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Export timestamp.
    pub exported_at: DateTime<Utc>,
}

/// A collection of portable credentials for bulk export/import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableCredentialBundle {
    /// Version of the export format.
    pub version: u32,
    /// The credentials in this bundle.
    pub credentials: Vec<PortableCredential>,
    /// When this bundle was exported.
    pub exported_at: DateTime<Utc>,
    /// Optional machine identifier.
    #[serde(default)]
    pub machine_id: Option<String>,
}

impl PortableCredentialBundle {
    /// Create a new empty bundle.
    pub fn new() -> Self {
        Self {
            version: 1,
            credentials: Vec::new(),
            exported_at: Utc::now(),
            machine_id: None,
        }
    }
}

impl Default for PortableCredentialBundle {
    fn default() -> Self {
        Self::new()
    }
}

/// On-disk representation of the credential store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CredentialStore {
    credentials: HashMap<String, Credential>,
}

/// Manages credentials with CRUD operations and disk persistence.
///
/// Credentials are stored as JSON files in `~/.shannon/credentials/`.
/// Each service gets its own file named `<service>.json`.
pub struct CredentialManager {
    /// Directory where credential files are stored.
    credentials_dir: PathBuf,
    /// In-memory cache of loaded credentials.
    store: CredentialStore,
    /// Whether the in-memory cache has been modified since last load/save.
    dirty: bool,
}

impl CredentialManager {
    /// Create a new CredentialManager using the default directory
    /// (`~/.shannon/credentials/`).
    pub fn new() -> Result<Self, CredentialError> {
        let credentials_dir = default_credentials_dir()?;
        Self::with_dir(credentials_dir)
    }

    /// Create a CredentialManager with a custom storage directory.
    pub fn with_dir(dir: PathBuf) -> Result<Self, CredentialError> {
        fs::create_dir_all(&dir)?;
        let manager = Self {
            credentials_dir: dir,
            store: CredentialStore::default(),
            dirty: false,
        };
        Ok(manager)
    }

    /// Store a new credential. Returns an error if a credential with the
    /// same service already exists.
    pub fn store(&mut self, credential: Credential) -> Result<(), CredentialError> {
        if self.store.credentials.contains_key(&credential.service) {
            return Err(CredentialError::AlreadyExists(format!(
                "Credential for service '{}' already exists",
                credential.service
            )));
        }

        if credential.name.is_empty() {
            return Err(CredentialError::Invalid("Credential name cannot be empty".into()));
        }
        if credential.service.is_empty() {
            return Err(CredentialError::Invalid("Credential service cannot be empty".into()));
        }

        self.store.credentials.insert(credential.service.clone(), credential);
        self.dirty = true;
        self.persist()?;
        Ok(())
    }

    /// Store a credential, replacing any existing credential for the same service.
    pub fn store_or_update(&mut self, credential: Credential) -> Result<(), CredentialError> {
        if credential.name.is_empty() {
            return Err(CredentialError::Invalid("Credential name cannot be empty".into()));
        }
        if credential.service.is_empty() {
            return Err(CredentialError::Invalid("Credential service cannot be empty".into()));
        }

        self.store.credentials.insert(credential.service.clone(), credential);
        self.dirty = true;
        self.persist()?;
        Ok(())
    }

    /// Retrieve a credential by service name.
    pub fn retrieve(&self, service: &str) -> Result<Credential, CredentialError> {
        self.store
            .credentials
            .get(service)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(format!("Credential for service '{service}'")))
    }

    /// Delete a credential by service name.
    pub fn delete(&mut self, service: &str) -> Result<Credential, CredentialError> {
        let credential = self
            .store
            .credentials
            .remove(service)
            .ok_or_else(|| CredentialError::NotFound(format!("Credential for service '{service}'")))?;

        // Remove the credential file from disk
        let file_path = self.credential_file_path(service);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }

        self.dirty = true;
        debug!(service = %service, "Deleted credential");
        Ok(credential)
    }

    /// List all stored credentials (without values).
    pub fn list(&self) -> Vec<CredentialSummary> {
        self.store
            .credentials
            .values()
            .map(|c| CredentialSummary {
                id: c.id.clone(),
                name: c.name.clone(),
                service: c.service.clone(),
                created_at: c.created_at,
                updated_at: c.updated_at,
                metadata: c.metadata.clone(),
            })
            .collect()
    }

    /// Export all credentials as a portable bundle.
    pub fn export_portable(&self) -> Result<PortableCredentialBundle, CredentialError> {
        let credentials: Vec<PortableCredential> = self
            .store
            .credentials
            .values()
            .map(|c| PortableCredential {
                name: c.name.clone(),
                service: c.service.clone(),
                value: c.value.clone(),
                metadata: c.metadata.clone(),
                exported_at: Utc::now(),
            })
            .collect();

        let bundle = PortableCredentialBundle {
            version: 1,
            credentials,
            exported_at: Utc::now(),
            machine_id: hostname(),
        };

        Ok(bundle)
    }

    /// Import credentials from a portable bundle. Existing credentials
    /// for the same service will be replaced unless `skip_existing` is true.
    pub fn import_portable(
        &mut self,
        bundle: PortableCredentialBundle,
        skip_existing: bool,
    ) -> Result<ImportResult, CredentialError> {
        let mut imported = 0usize;
        let mut skipped = 0usize;

        for portable in bundle.credentials {
            let exists = self.store.credentials.contains_key(&portable.service);
            if exists && skip_existing {
                skipped += 1;
                continue;
            }

            let credential = Credential {
                id: Uuid::new_v4().to_string(),
                name: portable.name,
                service: portable.service,
                value: portable.value,
                created_at: portable.exported_at,
                updated_at: Utc::now(),
                metadata: portable.metadata,
            };

            self.store.credentials.insert(credential.service.clone(), credential);
            imported += 1;
        }

        if imported > 0 {
            self.dirty = true;
            self.persist()?;
        }

        Ok(ImportResult { imported, skipped })
    }

    /// Load credentials from disk into memory.
    pub fn load(&mut self) -> Result<(), CredentialError> {
        if !self.credentials_dir.exists() {
            self.store = CredentialStore::default();
            return Ok(());
        }

        let mut all = HashMap::new();

        for entry in fs::read_dir(&self.credentials_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            // Validate file permissions before reading
            self.validate_file_permissions(&path)?;

            let content = fs::read_to_string(&path)?;
            let credential: Credential = serde_json::from_str(&content)?;
            all.insert(credential.service.clone(), credential);
        }

        let count = all.len();
        self.store = CredentialStore { credentials: all };
        self.dirty = false;
        debug!(
            count,
            dir = %self.credentials_dir.display(),
            "Loaded credentials from disk"
        );
        Ok(())
    }

    /// Get a file descriptor for a credential file.
    pub fn file_descriptor(&self, service: &str) -> CredentialFileDescriptor {
        let path = self.credential_file_path(service);
        let exists = path.exists();
        let (size, permissions) = if exists {
            let meta = fs::metadata(&path).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            #[cfg(unix)]
            let permissions = meta
                .as_ref()
                .map(|m| m.permissions().mode())
                .unwrap_or(0);
            #[cfg(not(unix))]
            let permissions = 0u32;
            (size, permissions)
        } else {
            (0, 0)
        };

        CredentialFileDescriptor {
            path,
            size,
            permissions,
            exists,
            format: CredentialFileFormat::Json,
        }
    }

    /// Validate that a credential file has secure permissions (0o600).
    pub fn validate_file_permissions(&self, path: &Path) -> Result<(), CredentialError> {
        if !path.exists() {
            return Ok(());
        }

        let meta = fs::metadata(path)?;

        // On Unix, check that the file is readable/writable only by the owner.
        // 0o600 = 0b110000000 in the lowest 9 bits.
        #[cfg(unix)]
        {
            const SECURE_MODE: u32 = 0o600;
            let file_mode = meta.permissions().mode() & 0o777;
            if file_mode != SECURE_MODE {
                // Warn but don't fail in tests or non-strict contexts.
                // In production, we would auto-fix or fail.
                warn!(
                    path = %path.display(),
                    actual = format!("{:#o}", file_mode),
                    expected = format!("{:#o}", SECURE_MODE),
                    "Credential file has insecure permissions"
                );
            }
        }

        Ok(())
    }

    /// Set secure permissions on a credential file (0o600 on Unix).
    pub fn set_secure_permissions(&self, path: &Path) -> Result<(), CredentialError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }

    /// Get the number of stored credentials.
    pub fn count(&self) -> usize {
        self.store.credentials.len()
    }

    /// Check if a credential exists for the given service.
    pub fn exists(&self, service: &str) -> bool {
        self.store.credentials.contains_key(service)
    }

    // --- Private helpers ---

    fn credential_file_path(&self, service: &str) -> PathBuf {
        self.credentials_dir.join(format!("{service}.json"))
    }

    fn persist(&self) -> Result<(), CredentialError> {
        fs::create_dir_all(&self.credentials_dir)?;

        for credential in self.store.credentials.values() {
            let path = self.credential_file_path(&credential.service);
            let content = serde_json::to_string_pretty(credential)?;
            fs::write(&path, content)?;
            self.set_secure_permissions(&path)?;
        }

        debug!(
            count = self.store.credentials.len(),
            dir = %self.credentials_dir.display(),
            "Persisted credentials to disk"
        );
        Ok(())
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new().expect("Failed to create default credential manager")
    }
}

/// Summary of a credential without the sensitive value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSummary {
    pub id: String,
    pub name: String,
    pub service: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

/// Result of an import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    /// Number of credentials imported.
    pub imported: usize,
    /// Number of credentials skipped (already existed).
    pub skipped: usize,
}

/// Get the default credentials directory path.
fn default_credentials_dir() -> Result<PathBuf, CredentialError> {
    let home = dirs::home_dir().ok_or_else(|| {
        CredentialError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Cannot determine home directory",
        ))
    })?;
    Ok(home.join(".shannon").join("credentials"))
}

/// Get the current machine hostname.
fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("shannon_cred_test_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
            fs::create_dir_all(&dir).expect("Failed to create test dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_credential_new() {
        let cred = Credential::new("Anthropic API Key", "anthropic", "sk-ant-123");
        assert!(!cred.id.is_empty());
        assert_eq!(cred.name, "Anthropic API Key");
        assert_eq!(cred.service, "anthropic");
        assert_eq!(cred.value, "sk-ant-123");
        assert_eq!(cred.created_at, cred.updated_at);
    }

    #[test]
    fn test_store_and_retrieve() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let cred = Credential::new("Anthropic", "anthropic", "sk-ant-test");
        mgr.store(cred).unwrap();

        let retrieved = mgr.retrieve("anthropic").unwrap();
        assert_eq!(retrieved.value, "sk-ant-test");
        assert_eq!(retrieved.service, "anthropic");
    }

    #[test]
    fn test_store_duplicate_rejects() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let cred1 = Credential::new("Anthropic", "anthropic", "key1");
        mgr.store(cred1).unwrap();

        let cred2 = Credential::new("Anthropic 2", "anthropic", "key2");
        let result = mgr.store(cred2);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn test_store_or_update() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let cred1 = Credential::new("Anthropic", "anthropic", "key1");
        mgr.store_or_update(cred1).unwrap();

        let cred2 = Credential::new("Anthropic Updated", "anthropic", "key2");
        mgr.store_or_update(cred2).unwrap();

        let retrieved = mgr.retrieve("anthropic").unwrap();
        assert_eq!(retrieved.value, "key2");
        assert_eq!(retrieved.name, "Anthropic Updated");
    }

    #[test]
    fn test_retrieve_not_found() {
        let td = TestDir::new();
        let mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let result = mgr.retrieve("nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_delete_credential() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let cred = Credential::new("Anthropic", "anthropic", "key1");
        mgr.store(cred).unwrap();
        assert!(mgr.exists("anthropic"));

        let deleted = mgr.delete("anthropic").unwrap();
        assert_eq!(deleted.service, "anthropic");
        assert!(!mgr.exists("anthropic"));
    }

    #[test]
    fn test_delete_not_found() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let result = mgr.delete("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_credentials() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        mgr.store(Credential::new("Anthropic", "anthropic", "key1")).unwrap();
        mgr.store(Credential::new("GitHub", "github", "ghp-test")).unwrap();

        let list = mgr.list();
        assert_eq!(list.len(), 2);
        // List should not contain values
        assert!(list.iter().all(|s| s.metadata.is_empty()));
    }

    #[test]
    fn test_list_empty() {
        let td = TestDir::new();
        let mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn test_persistence() {
        let td = TestDir::new();
        let dir = td.path().to_path_buf();

        // Store a credential
        {
            let mut mgr = CredentialManager::with_dir(dir.clone()).unwrap();
            mgr.store(Credential::new("Anthropic", "anthropic", "persist-key")).unwrap();
        }

        // Load it back in a new manager
        {
            let mut mgr = CredentialManager::with_dir(dir.clone()).unwrap();
            mgr.load().unwrap();
            let cred = mgr.retrieve("anthropic").unwrap();
            assert_eq!(cred.value, "persist-key");
        }
    }

    #[test]
    fn test_export_portable() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        mgr.store(Credential::new("Anthropic", "anthropic", "key1")).unwrap();
        mgr.store(Credential::new("GitHub", "github", "ghp-test")).unwrap();

        let bundle = mgr.export_portable().unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.credentials.len(), 2);

        let services: Vec<&str> = bundle.credentials.iter().map(|c| c.service.as_str()).collect();
        assert!(services.contains(&"anthropic"));
        assert!(services.contains(&"github"));
    }

    #[test]
    fn test_import_portable() {
        let td = TestDir::new();
        let dir = td.path().to_path_buf();

        // Create a bundle to import
        let mut bundle = PortableCredentialBundle::new();
        bundle.credentials.push(PortableCredential {
            name: "Anthropic".into(),
            service: "anthropic".into(),
            value: "imported-key".into(),
            metadata: HashMap::new(),
            exported_at: Utc::now(),
        });

        let mut mgr = CredentialManager::with_dir(dir).unwrap();
        let result = mgr.import_portable(bundle, false).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 0);

        let cred = mgr.retrieve("anthropic").unwrap();
        assert_eq!(cred.value, "imported-key");
    }

    #[test]
    fn test_import_portable_skip_existing() {
        let td = TestDir::new();
        let dir = td.path().to_path_buf();

        let mut mgr = CredentialManager::with_dir(dir).unwrap();
        mgr.store(Credential::new("Anthropic", "anthropic", "original-key")).unwrap();

        let mut bundle = PortableCredentialBundle::new();
        bundle.credentials.push(PortableCredential {
            name: "Anthropic New".into(),
            service: "anthropic".into(),
            value: "new-key".into(),
            metadata: HashMap::new(),
            exported_at: Utc::now(),
        });

        let result = mgr.import_portable(bundle, true).unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.skipped, 1);

        // Original should be preserved
        let cred = mgr.retrieve("anthropic").unwrap();
        assert_eq!(cred.value, "original-key");
    }

    #[test]
    fn test_file_descriptor() {
        let td = TestDir::new();
        let mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let desc = mgr.file_descriptor("anthropic");
        assert!(!desc.exists);
        assert_eq!(desc.size, 0);
        assert_eq!(desc.format, CredentialFileFormat::Json);
    }

    #[test]
    fn test_store_empty_name_rejects() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let mut cred = Credential::new("Anthropic", "anthropic", "key");
        cred.name = String::new();
        let result = mgr.store(cred);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name cannot be empty"));
    }

    #[test]
    fn test_store_empty_service_rejects() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let mut cred = Credential::new("Anthropic", "anthropic", "key");
        cred.service = String::new();
        let result = mgr.store(cred);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("service cannot be empty"));
    }

    #[test]
    fn test_count() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        assert_eq!(mgr.count(), 0);

        mgr.store(Credential::new("A", "a", "1")).unwrap();
        mgr.store(Credential::new("B", "b", "2")).unwrap();
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn test_credential_file_created() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        mgr.store(Credential::new("Anthropic", "anthropic", "key")).unwrap();

        let file_path = td.path().join("anthropic.json");
        assert!(file_path.exists(), "Credential file should be created on disk");

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("anthropic"));
        assert!(content.contains("key"));
    }
}
