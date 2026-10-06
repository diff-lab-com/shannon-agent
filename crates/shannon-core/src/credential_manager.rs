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
use tracing::debug;
#[cfg(unix)]
use tracing::warn;
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

    #[error(
        "Permission error on credential file: {path} has mode {actual:#o}, expected {expected:#o}"
    )]
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
///
/// R4-3 (multi-key rotation): a credential document may hold MORE than one
/// key for its service. The storage invariant is deliberately simple:
///
/// > **The active key is always slot 0** (`value`); `extra_values` holds the
/// > remaining keys in stable list order.
///
/// Consequences of that invariant:
/// - pre-R4-3 single-value files parse unchanged (the new field defaults to
///   empty and is omitted when empty) and keep their exact byte shape on
///   rewrite;
/// - an OLD reader (any binary predating this change) that reads `.value`
///   always gets the **active** key — no stale-key surprises across versions;
/// - resolution is deterministic: the rotation order is exactly the file
///   order `[value] ++ extra_values` (see [`Credential::keys_in_rotation_order`]);
/// - "which key is active" is observable as list position 0 — there is no
///   separate `active` index to drift out of sync (`activate` physically
///   swaps slot 0 with the requested slot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    /// Unique identifier for this credential.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Service this credential is for (e.g., "anthropic", "github").
    pub service: String,
    /// The ACTIVE credential value (stored in plaintext on disk; encryption
    /// is the caller's responsibility before calling `store`). Slot 0 of the
    /// multi-key list — see the struct docs.
    pub value: String,
    /// R4-3: the remaining API keys (slots 1..n) in stable list order.
    /// Rotation tries these in order after `value`. Empty = single-key
    /// credential (the historical shape, also what old files parse to).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_values: Vec<String>,
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
            extra_values: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    /// The full key list in rotation order: the active key (slot 0, `value`)
    /// first, then `extra_values`. Empty values are skipped — a blank slot
    /// can never be rotated to.
    pub fn keys_in_rotation_order(&self) -> Vec<String> {
        let mut keys = Vec::with_capacity(1 + self.extra_values.len());
        if !self.value.is_empty() {
            keys.push(self.value.clone());
        }
        keys.extend(self.extra_values.iter().filter(|k| !k.is_empty()).cloned());
        keys
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

/// R4-3 merge rule for "set the key" writes (`store_or_update`, portable
/// import): the incoming value replaces the ACTIVE key while the existing
/// rotation list survives, minus duplicates of the incoming value (and blank
/// slots). See the [`Credential`] storage invariant.
fn preserve_extra_keys(existing: &Credential, incoming: &mut Credential) {
    incoming.extra_values = existing
        .extra_values
        .iter()
        .filter(|k| k.as_str() != incoming.value && !k.is_empty())
        .cloned()
        .collect();
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

    /// The directory this manager persists to (`~/.shannon/credentials` for
    /// the default constructor). R4-3: lets callers resolve `store:SERVICE`
    /// key references against the SAME store the manager reads and writes.
    pub fn dir(&self) -> &Path {
        &self.credentials_dir
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
            return Err(CredentialError::Invalid(
                "Credential name cannot be empty".into(),
            ));
        }
        if credential.service.is_empty() {
            return Err(CredentialError::Invalid(
                "Credential service cannot be empty".into(),
            ));
        }

        self.store
            .credentials
            .insert(credential.service.clone(), credential);
        self.dirty = true;
        self.persist()?;
        Ok(())
    }

    /// Store a credential, replacing any existing credential for the same service.
    ///
    /// R4-3: when the service already has a multi-key entry and the incoming
    /// credential carries NO extras of its own (the shape every "set the
    /// key" caller produces via [`Credential::new`]), the stored **extra**
    /// keys survive the overwrite — `/connect`/`/credentials` replace the
    /// ACTIVE key while the rotation list stays intact, minus duplicates of
    /// the new value. Callers that deliberately construct a full key list
    /// (the multi-key operations below) bypass the merge via
    /// `Self::persist_credential`.
    pub fn store_or_update(&mut self, credential: Credential) -> Result<(), CredentialError> {
        Self::validate_credential(&credential)?;
        let mut credential = credential;
        if credential.extra_values.is_empty() {
            if let Some(existing) = self.store.credentials.get(&credential.service) {
                preserve_extra_keys(existing, &mut credential);
            }
        }
        self.persist_credential(credential)
    }

    /// Validate + insert + persist WITHOUT the store_or_update merge. The
    /// multi-key operations (`add_key` / `remove_key` / `activate_key`)
    /// build the full new key list themselves and must not have it
    /// recomputed from the old entry.
    fn persist_credential(&mut self, credential: Credential) -> Result<(), CredentialError> {
        Self::validate_credential(&credential)?;
        self.store
            .credentials
            .insert(credential.service.clone(), credential);
        self.dirty = true;
        self.persist()?;
        Ok(())
    }

    fn validate_credential(credential: &Credential) -> Result<(), CredentialError> {
        if credential.name.is_empty() {
            return Err(CredentialError::Invalid(
                "Credential name cannot be empty".into(),
            ));
        }
        if credential.service.is_empty() {
            return Err(CredentialError::Invalid(
                "Credential service cannot be empty".into(),
            ));
        }
        Ok(())
    }

    /// The stored keys for `service` in rotation order (active key first).
    /// Errors with [`CredentialError::NotFound`] when the service has no
    /// entry.
    pub fn keys(&self, service: &str) -> Result<Vec<String>, CredentialError> {
        self.retrieve(service).map(|c| c.keys_in_rotation_order())
    }

    /// R4-3: append a key to `service`'s multi-key list. Refuses duplicates
    /// (the value is already registered — the error names its slot) and empty
    /// values. Returns the new total key count.
    pub fn add_key(&mut self, service: &str, value: &str) -> Result<usize, CredentialError> {
        if value.trim().is_empty() {
            return Err(CredentialError::Invalid("cannot add an empty key".into()));
        }
        let mut credential = self.retrieve(service)?;
        let existing = credential.keys_in_rotation_order();
        if let Some(pos) = existing.iter().position(|k| k == value) {
            return Err(CredentialError::AlreadyExists(format!(
                "key is already registered at slot {pos}"
            )));
        }
        credential.extra_values.push(value.to_string());
        credential.updated_at = Utc::now();
        let count = 1 + credential.extra_values.len();
        self.persist_credential(credential)?;
        Ok(count)
    }

    /// R4-3: remove the key at `index` (0-based position in the rotation
    /// order shown by [`Self::keys`] / the CLI). Removing slot 0 promotes the
    /// next stored key to active; the LAST remaining key cannot be removed
    /// (a credential entry always holds at least one — delete the whole
    /// entry with [`Self::delete`] instead).
    pub fn remove_key(&mut self, service: &str, index: usize) -> Result<(), CredentialError> {
        let mut credential = self.retrieve(service)?;
        let count = 1 + credential.extra_values.len();
        if index >= count {
            return Err(CredentialError::Invalid(format!(
                "key index {index} out of range (service '{service}' has {count} keys)"
            )));
        }
        if count == 1 {
            return Err(CredentialError::Invalid(
                "cannot remove the last remaining key; delete the credential instead".into(),
            ));
        }
        if index == 0 {
            // Promote the next stored key to active (slot 0).
            credential.value = credential.extra_values.remove(0);
        } else {
            credential.extra_values.remove(index - 1);
        }
        credential.updated_at = Utc::now();
        self.persist_credential(credential)?;
        Ok(())
    }

    /// R4-3: make the key at `index` the ACTIVE key. `activate(0)` is a
    /// no-op; any other index swaps slot 0 with slot `index`, preserving the
    /// storage invariant "the active key is always `value`" (which is also
    /// what pre-R4-3 readers observe).
    pub fn activate_key(&mut self, service: &str, index: usize) -> Result<(), CredentialError> {
        let mut credential = self.retrieve(service)?;
        let count = 1 + credential.extra_values.len();
        if index >= count {
            return Err(CredentialError::Invalid(format!(
                "key index {index} out of range (service '{service}' has {count} keys)"
            )));
        }
        if index != 0 {
            credential.extra_values.swap(index - 1, 0);
            std::mem::swap(&mut credential.value, &mut credential.extra_values[0]);
        }
        credential.updated_at = Utc::now();
        self.persist_credential(credential)?;
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
        let credential = self.store.credentials.remove(service).ok_or_else(|| {
            CredentialError::NotFound(format!("Credential for service '{service}'"))
        })?;

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

            let mut credential = Credential {
                id: Uuid::new_v4().to_string(),
                name: portable.name,
                service: portable.service,
                value: portable.value,
                extra_values: Vec::new(),
                created_at: portable.exported_at,
                updated_at: Utc::now(),
                metadata: portable.metadata,
            };
            if let Some(existing) = self.store.credentials.get(&credential.service) {
                preserve_extra_keys(existing, &mut credential);
            }

            self.store
                .credentials
                .insert(credential.service.clone(), credential);
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
            let permissions = meta.as_ref().map(|m| m.permissions().mode()).unwrap_or(0);
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

        // On Unix, check that the file is readable/writable only by the owner.
        // 0o600 = 0b110000000 in the lowest 9 bits.
        #[cfg(unix)]
        {
            let meta = fs::metadata(path)?;
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
        #[cfg(not(unix))]
        let _ = path; // POSIX file modes do not exist on this platform
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
        let safe_name = service.replace(['/', '\\', '\0'], "_");
        self.credentials_dir.join(format!("{safe_name}.json"))
    }

    fn persist(&self) -> Result<(), CredentialError> {
        fs::create_dir_all(&self.credentials_dir)?;

        for credential in self.store.credentials.values() {
            let path = self.credential_file_path(&credential.service);
            let content = serde_json::to_string_pretty(credential)?;
            atomic_write_secure(&path, &content)?;
        }

        debug!(
            count = self.store.credentials.len(),
            dir = %self.credentials_dir.display(),
            "Persisted credentials to disk"
        );
        Ok(())
    }
}

/// Atomically write `content` to `path` via a temp file + rename (ADR-0005
/// Phase 1, P3-2).
///
/// Owner-only permissions (0600 on Unix) are set on the **temp** file before
/// the rename, so the final path is never observable in a world-readable
/// state and a crash mid-write cannot leave a partial credential file. The
/// rename is atomic on the same filesystem; the temp file is cleaned up by
/// the rename itself on success (a crash before rename may leave a stale
/// `<service>.json.tmp`, which is harmless and ignored by readers).
fn atomic_write_secure(path: &Path, content: &str) -> Result<(), CredentialError> {
    // review §P2-25: the previous version did `fs::write` (which creates
    // the file with the process umask, typically 0644) and only THEN
    // chmod'd it to 0600. There is a brief window during which the
    // plaintext credentials sit in a world-readable file on disk, which
    // violates the doc comment above ("never observable in a world-
    // readable state"). Open with O_CREAT | mode 0o600 on unix so the
    // file is created with the right permissions in one syscall.
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&tmp, content)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

impl Default for CredentialManager {
    fn default() -> Self {
        let credentials_dir = dirs::home_dir()
            .unwrap_or_else(|| {
                eprintln!("Warning: Home directory not found, using /tmp");
                std::path::PathBuf::from("/tmp")
            })
            .join(".shannon")
            .join("credentials");
        Self::with_dir(credentials_dir).unwrap_or_else(|first_err| {
            tracing::error!("CredentialManager: home dir failed: {first_err}");
            let fallback = std::env::temp_dir().join(".shannon").join("credentials");
            Self::with_dir(fallback.clone()).unwrap_or_else(|second_err| {
                tracing::error!("CredentialManager: temp dir failed: {second_err}");
                let last_resort = PathBuf::from("/tmp/.shannon/credentials");
                match Self::with_dir(last_resort.clone()) {
                    Ok(s) => s,
                    Err(third_err) => {
                        tracing::error!("CredentialManager: all fallbacks failed: {third_err}. Using non-persisting instance.");
                        Self {
                            credentials_dir: fallback,
                            store: CredentialStore::default(),
                            dirty: false,
                        }
                    }
                }
            })
        })
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

/// Read a single credential's value directly from its on-disk file in `dir`,
/// bypassing the in-memory cache.
///
/// This is the hot-path read used by the provider resolver (ADR-0005 Phase 1)
/// so the request path can honor [`CredentialRef::Store { service }`] without
/// constructing a full [`CredentialManager`] and running a `load()` cycle.
/// Returns `None` when the file is missing, unreadable, or not a valid
/// credential document.
///
/// [`CredentialRef::Store { service }`]: shannon_types::provider_config::CredentialRef::Store
pub fn read_credential_value(dir: &Path, service: &str) -> Option<String> {
    let credential = read_credential_document(dir, service)?;
    Some(credential.value)
}

/// Read a credential value from the default store directory
/// (`~/.shannon/credentials/`). Returns `None` when the home directory cannot
/// be determined or the credential is absent/unreadable.
pub fn read_credential_value_default(service: &str) -> Option<String> {
    let dir = default_credentials_dir().ok()?;
    read_credential_value(&dir, service)
}

/// R4-3: read ALL keys stored for `service` from its on-disk file in `dir`,
/// in rotation order (active key first) — the multi-key counterpart of
/// [`read_credential_value`]. Returns `None` when the file is missing or not
/// a valid credential document; single-key credentials yield a one-element
/// Vec.
pub fn read_credential_keys(dir: &Path, service: &str) -> Option<Vec<String>> {
    let credential = read_credential_document(dir, service)?;
    Some(credential.keys_in_rotation_order())
}

/// Read a credential value from the default store directory
/// (`~/.shannon/credentials/`) — multi-key counterpart of
/// [`read_credential_value_default`].
pub fn read_credential_keys_default(service: &str) -> Option<Vec<String>> {
    let dir = default_credentials_dir().ok()?;
    read_credential_keys(&dir, service)
}

/// Read the full credential document for `service` directly from its on-disk
/// file in `dir` (bypassing any in-memory cache).
fn read_credential_document(dir: &Path, service: &str) -> Option<Credential> {
    let safe_name = service.replace(['/', '\\', '\0'], "_");
    let path = dir.join(format!("{safe_name}.json"));
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Get the current machine hostname.
fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "shannon_cred_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
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

        mgr.store(Credential::new("Anthropic", "anthropic", "key1"))
            .unwrap();
        mgr.store(Credential::new("GitHub", "github", "ghp-test"))
            .unwrap();

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
            mgr.store(Credential::new("Anthropic", "anthropic", "persist-key"))
                .unwrap();
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

        mgr.store(Credential::new("Anthropic", "anthropic", "key1"))
            .unwrap();
        mgr.store(Credential::new("GitHub", "github", "ghp-test"))
            .unwrap();

        let bundle = mgr.export_portable().unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.credentials.len(), 2);

        let services: Vec<&str> = bundle
            .credentials
            .iter()
            .map(|c| c.service.as_str())
            .collect();
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
        mgr.store(Credential::new("Anthropic", "anthropic", "original-key"))
            .unwrap();

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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("name cannot be empty")
        );
    }

    #[test]
    fn test_store_empty_service_rejects() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();

        let mut cred = Credential::new("Anthropic", "anthropic", "key");
        cred.service = String::new();
        let result = mgr.store(cred);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("service cannot be empty")
        );
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

        mgr.store(Credential::new("Anthropic", "anthropic", "key"))
            .unwrap();

        let file_path = td.path().join("anthropic.json");
        assert!(
            file_path.exists(),
            "Credential file should be created on disk"
        );

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("anthropic"));
        assert!(content.contains("key"));
    }

    // ── read_credential_value (ADR-0005 Phase 1 hot path) ──────────────

    #[test]
    fn test_read_credential_value_present() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        mgr.store_or_update(Credential::new("Anthropic", "anthropic", "sk-from-store"))
            .unwrap();

        // Direct disk read — no load() needed.
        assert_eq!(
            read_credential_value(td.path(), "anthropic"),
            Some("sk-from-store".to_string())
        );
    }

    #[test]
    fn test_read_credential_value_missing_is_none() {
        let td = TestDir::new();
        assert!(read_credential_value(td.path(), "ghost").is_none());
    }

    #[test]
    fn test_read_credential_value_corrupt_is_none() {
        let td = TestDir::new();
        // Write a malformed JSON file where a credential is expected.
        fs::write(td.path().join("anthropic.json"), "{ not json").unwrap();
        assert!(read_credential_value(td.path(), "anthropic").is_none());
    }

    #[test]
    fn test_read_credential_value_sanitizes_service_name() {
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        // A service with path separators is stored on disk under a sanitized
        // name; the reader must apply the same sanitization to find it.
        mgr.store_or_update(Credential::new("Zhipu", "zhipu/coding", "glm-key"))
            .unwrap();
        assert_eq!(
            read_credential_value(td.path(), "zhipu/coding"),
            Some("glm-key".to_string())
        );
    }

    #[test]
    fn test_read_credential_value_picks_up_external_write() {
        // Simulates `/connect` writing a credential file after the process
        // started: the direct read must see it without any in-memory cache.
        let td = TestDir::new();
        let cred = Credential::new("Anthropic", "anthropic", "freshly-written");
        fs::write(
            td.path().join("anthropic.json"),
            serde_json::to_string(&cred).unwrap(),
        )
        .unwrap();

        assert_eq!(
            read_credential_value(td.path(), "anthropic"),
            Some("freshly-written".to_string())
        );
    }

    // ── atomic_write_secure (ADR-0005 Phase 1, P3-2) ───────────────────

    #[test]
    fn test_atomic_write_leaves_no_tmp_and_correct_content() {
        let td = TestDir::new();
        let path = td.path().join("anthropic.json");
        atomic_write_secure(&path, "{\"service\":\"anthropic\"}").unwrap();

        // Final file exists with the content…
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"service\":\"anthropic\"}"
        );
        // …and no temp file is left behind.
        assert!(!td.path().join("anthropic.json.tmp").exists());
    }

    #[test]
    fn test_atomic_write_sets_owner_only_permissions_on_unix() {
        let td = TestDir::new();
        let path = td.path().join("deepseek.json");
        atomic_write_secure(&path, "x").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "credential file must be owner-only (0600)");
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }

    #[test]
    fn test_store_persist_is_atomic_via_manager() {
        // End-to-end: storing through the manager must land a complete file
        // with no leftover .tmp (regression guard for the atomic persist).
        let td = TestDir::new();
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        mgr.store(Credential::new("Anthropic", "anthropic", "k"))
            .unwrap();
        assert!(td.path().join("anthropic.json").exists());
        assert!(!td.path().join("anthropic.json.tmp").exists());
    }

    // ── R4-3: multi-key storage + rotation order ────────────────────────

    fn multi_key_manager(dir: &Path) -> CredentialManager {
        let mut mgr = CredentialManager::with_dir(dir.to_path_buf()).unwrap();
        let mut cred = Credential::new("Anthropic", "anthropic", "key-a");
        cred.extra_values = vec!["key-b".to_string(), "key-c".to_string()];
        mgr.store(cred).unwrap();
        mgr
    }

    #[test]
    fn multi_key_rotation_order_is_active_first_then_list_order() {
        let td = TestDir::new();
        let mgr = multi_key_manager(td.path());
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-a", "key-b", "key-c"]
        );
    }

    #[test]
    fn multi_key_activate_swaps_active_to_front() {
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        mgr.activate_key("anthropic", 2).unwrap();
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-c", "key-a", "key-b"],
            "active first, remaining keys keep their prior list order"
        );
        // And back: activate(0) is a no-op, activate(1) swaps with slot 1.
        mgr.activate_key("anthropic", 0).unwrap();
        assert_eq!(mgr.keys("anthropic").unwrap()[0], "key-c");
        mgr.activate_key("anthropic", 1).unwrap();
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-a", "key-c", "key-b"]
        );
    }

    #[test]
    fn multi_key_activate_out_of_range_rejects() {
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        let err = mgr.activate_key("anthropic", 3).unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
        assert_eq!(mgr.keys("anthropic").unwrap().len(), 3, "no mutation");
    }

    #[test]
    fn multi_key_add_appends_and_dedupes() {
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        assert_eq!(mgr.add_key("anthropic", "key-d").unwrap(), 4);
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-a", "key-b", "key-c", "key-d"]
        );
        let err = mgr.add_key("anthropic", "key-b").unwrap_err();
        assert!(
            err.to_string().contains("already registered at slot 1"),
            "{err}"
        );
        assert!(mgr.add_key("anthropic", "  ").is_err(), "empty rejected");
    }

    #[test]
    fn multi_key_remove_splices_and_promotes_on_slot_zero() {
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        mgr.remove_key("anthropic", 1).unwrap();
        assert_eq!(mgr.keys("anthropic").unwrap(), vec!["key-a", "key-c"]);
        // Removing the active key promotes the next one.
        mgr.remove_key("anthropic", 0).unwrap();
        assert_eq!(mgr.keys("anthropic").unwrap(), vec!["key-c"]);
        // The last remaining key is protected.
        let err = mgr.remove_key("anthropic", 0).unwrap_err();
        assert!(err.to_string().contains("last remaining key"), "{err}");
        assert_eq!(mgr.keys("anthropic").unwrap(), vec!["key-c"]);
    }

    #[test]
    fn multi_key_remove_out_of_range_rejects() {
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        let err = mgr.remove_key("anthropic", 7).unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    #[test]
    fn multi_key_store_or_update_replaces_active_preserves_extras() {
        // /connect re-typing a key replaces the ACTIVE key but must not wipe
        // the rotation list.
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        mgr.store_or_update(Credential::new("Anthropic", "anthropic", "key-new"))
            .unwrap();
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-new", "key-b", "key-c"],
            "old active replaced, extras preserved, no duplicate of the new key"
        );
        // Re-typing a key that is ALREADY an extra: it must not appear twice.
        mgr.store_or_update(Credential::new("Anthropic", "anthropic", "key-b"))
            .unwrap();
        assert_eq!(mgr.keys("anthropic").unwrap(), vec!["key-b", "key-c"]);
    }

    #[test]
    fn multi_key_persists_across_manager_reload() {
        let td = TestDir::new();
        {
            let mut mgr = multi_key_manager(td.path());
            mgr.activate_key("anthropic", 1).unwrap();
        }
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        mgr.load().unwrap();
        assert_eq!(
            mgr.keys("anthropic").unwrap(),
            vec!["key-b", "key-a", "key-c"]
        );
    }

    #[test]
    fn legacy_single_value_file_parses_and_reads_unchanged() {
        // A pre-R4-3 credential file (no `extra_values` field) must parse and
        // behave byte-compatibly: one key, active, and `read_credential_value`
        // keeps returning it.
        let td = TestDir::new();
        std::fs::write(
            td.path().join("anthropic.json"),
            r#"{"id":"t1","name":"anthropic","service":"anthropic","value":"legacy-key","created_at":"2026-08-21T00:00:00Z","updated_at":"2026-08-21T00:00:00Z","metadata":{}}"#,
        )
        .unwrap();

        assert_eq!(
            read_credential_value(td.path(), "anthropic"),
            Some("legacy-key".to_string())
        );
        assert_eq!(
            read_credential_keys(td.path(), "anthropic"),
            Some(vec!["legacy-key".to_string()]),
            "single-key file yields exactly one rotation slot"
        );

        // A round-trip through the manager must NOT grow an empty
        // `extra_values` field (single-key files keep their exact shape).
        let mut mgr = CredentialManager::with_dir(td.path().to_path_buf()).unwrap();
        mgr.load().unwrap();
        mgr.activate_key("anthropic", 0).unwrap();
        let content = std::fs::read_to_string(td.path().join("anthropic.json")).unwrap();
        assert!(
            !content.contains("extra_values"),
            "single-key round-trip must stay shape-stable: {content}"
        );
    }

    #[test]
    fn multi_key_old_reader_sees_active_key_via_value_field() {
        // The compat contract: a pre-R4-3 reader reads `.value` and must get
        // the ACTIVE key, whatever the rotation state is.
        let td = TestDir::new();
        let mut mgr = multi_key_manager(td.path());
        mgr.activate_key("anthropic", 2).unwrap();
        // Direct disk read like an old binary would do.
        let content = std::fs::read_to_string(td.path().join("anthropic.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["value"], "key-c", "value is the active key");
        assert_eq!(
            read_credential_value(td.path(), "anthropic"),
            Some("key-c".to_string())
        );
    }

    #[test]
    fn read_credential_keys_missing_or_corrupt_is_none() {
        let td = TestDir::new();
        assert!(read_credential_keys(td.path(), "ghost").is_none());
        std::fs::write(td.path().join("bad.json"), "{ not json").unwrap();
        assert!(read_credential_keys(td.path(), "bad").is_none());
    }

    #[test]
    fn keys_in_rotation_order_skips_blank_slots() {
        let mut cred = Credential::new("S", "svc", "real-key");
        cred.extra_values = vec![String::new(), "second".to_string()];
        assert_eq!(
            cred.keys_in_rotation_order(),
            vec!["real-key".to_string(), "second".to_string()]
        );
    }
}
