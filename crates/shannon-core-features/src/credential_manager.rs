//! # Credential Manager
//!
//! Secure storage and management of credentials (API keys, tokens, passwords).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error
// ============================================================================

/// Errors that can occur during credential management.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Credential not found: {0}")]
    NotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Access denied")]
    AccessDenied,

    #[error("Invalid credential format")]
    InvalidFormat,
}

// ============================================================================
// Credential
// ============================================================================

/// A stored credential with service information and secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    /// Unique identifier for this credential.
    pub id: Uuid,

    /// Service name (e.g., "github", "openai", "aws").
    pub service: String,

    /// Username or account identifier.
    pub username: Option<String>,

    /// API token or bearer token.
    pub token: Option<String>,

    /// API key or secret.
    pub api_key: Option<String>,

    /// Additional metadata.
    pub metadata: HashMap<String, String>,

    /// When the credential was created.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When the credential was last accessed.
    pub accessed_at: chrono::DateTime<chrono::Utc>,
}

impl Credential {
    /// Create a new credential.
    pub fn new(service: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            service,
            username: None,
            token: None,
            api_key: None,
            metadata: HashMap::new(),
            created_at: now,
            accessed_at: now,
        }
    }

    /// Set the username.
    pub fn with_username(mut self, username: String) -> Self {
        self.username = Some(username);
        self
    }

    /// Set the token.
    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    /// Set the API key.
    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Mark the credential as accessed.
    pub fn touch(&mut self) {
        self.accessed_at = chrono::Utc::now();
    }

    /// Check if this credential has a token or API key.
    pub fn has_secret(&self) -> bool {
        self.token.is_some() || self.api_key.is_some()
    }
}

// ============================================================================
// Secure Storage Type
// ============================================================================

/// Type of secure storage backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecureStorage {
    /// System keyring (platform-specific).
    Keyring,

    /// Encrypted file storage.
    EncryptedFile,

    /// In-memory only (not persisted).
    Memory,
}

// ============================================================================
// Credential Manager
// ============================================================================

/// Manages secure storage and retrieval of credentials.
pub struct CredentialManager {
    credentials: HashMap<Uuid, Credential>,
    service_index: HashMap<String, Uuid>,
    storage_type: SecureStorage,
    storage_path: Option<PathBuf>,
}

impl CredentialManager {
    /// Create a new credential manager.
    pub fn new(storage_type: SecureStorage) -> Self {
        Self {
            credentials: HashMap::new(),
            service_index: HashMap::new(),
            storage_type,
            storage_path: None,
        }
    }

    /// Set the storage path for file-based storage.
    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Store a credential.
    pub fn store(&mut self, credential: Credential) -> Result<(), CredentialError> {
        let id = credential.id;
        let service = credential.service.clone();

        // Remove old credential for same service if exists
        if let Some(old_id) = self.service_index.get(&service) {
            self.credentials.remove(old_id);
        }

        self.credentials.insert(id, credential.clone());
        self.service_index.insert(service, id);

        // Persist to storage if configured
        if let Some(path) = &self.storage_path {
            self.persist(&credential, path)?;
        }

        Ok(())
    }

    /// Retrieve a credential by service name.
    pub fn retrieve(&self, service: &str) -> Result<Credential, CredentialError> {
        if let Some(id) = self.service_index.get(service) {
            Ok(self.credentials.get(id).cloned().unwrap_or_else(|| {
                // Fallback: shouldn't happen with consistent state
                Credential::new(service.to_string())
            }))
        } else {
            Err(CredentialError::NotFound(service.to_string()))
        }
    }

    /// Retrieve a credential by ID.
    pub fn retrieve_by_id(&self, id: &Uuid) -> Result<Credential, CredentialError> {
        self.credentials
            .get(id)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(id.to_string()))
    }

    /// List all credentials.
    pub fn list(&self) -> Vec<&Credential> {
        self.credentials.values().collect()
    }

    /// List all service names.
    pub fn list_services(&self) -> Vec<&String> {
        self.service_index.keys().collect()
    }

    /// Delete a credential by service name.
    pub fn delete(&mut self, service: &str) -> Result<(), CredentialError> {
        if let Some(id) = self.service_index.remove(service) {
            self.credentials.remove(&id);

            // Remove from storage if file-based
            if let Some(path) = &self.storage_path {
                let file_path = path.join(format!("{}.json", id));
                let _ = std::fs::remove_file(file_path);
            }

            Ok(())
        } else {
            Err(CredentialError::NotFound(service.to_string()))
        }
    }

    /// Delete a credential by ID.
    pub fn delete_by_id(&mut self, id: &Uuid) -> Result<(), CredentialError> {
        if let Some(credential) = self.credentials.remove(id) {
            self.service_index.remove(&credential.service);

            if let Some(path) = &self.storage_path {
                let file_path = path.join(format!("{}.json", id));
                let _ = std::fs::remove_file(file_path);
            }

            Ok(())
        } else {
            Err(CredentialError::NotFound(id.to_string()))
        }
    }

    /// Persist a credential to disk with secure permissions.
    fn persist(&self, credential: &Credential, path: &PathBuf) -> Result<(), CredentialError> {
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CredentialError::StorageError(e.to_string()))?;
        }

        // Serialize credential
        let json = serde_json::to_string(credential)
            .map_err(|e| CredentialError::EncryptionError(e.to_string()))?;

        // Write to a temporary file first
        let file_path = path.join(format!("{}.json", credential.id));
        let temp_path = path.join(format!("{}.json.tmp", credential.id));

        let mut file = fs::File::create(&temp_path)
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        file.write_all(json.as_bytes())
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        // Set secure permissions (read/write for owner only)
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        // Atomic rename
        fs::rename(&temp_path, &file_path)
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load all credentials from disk.
    pub fn load_from_disk(&mut self) -> Result<(), CredentialError> {
        if let Some(path) = &self.storage_path {
            if !path.exists() {
                return Ok(());
            }

            for entry in fs::read_dir(path)
                .map_err(|e| CredentialError::StorageError(e.to_string()))?
            {
                let entry = entry.map_err(|e| CredentialError::StorageError(e.to_string()))?;
                let file_path = entry.path();

                // Skip non-JSON files and temporary files
                if file_path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                if file_path.to_string_lossy().ends_with(".tmp") {
                    continue;
                }

                let json = fs::read_to_string(&file_path)
                    .map_err(|e| CredentialError::StorageError(e.to_string()))?;

                let credential: Credential = serde_json::from_str(&json)?;

                self.credentials.insert(credential.id, credential.clone());
                self.service_index.insert(credential.service.clone(), credential.id);
            }
        }

        Ok(())
    }

    /// Export credentials to an encrypted JSON file.
    pub fn export_encrypted(&self, password: &str, output_path: &PathBuf) -> Result<(), CredentialError> {
        let json = serde_json::to_string(&self.credentials)
            .map_err(|e| CredentialError::EncryptionError(e.to_string()))?;

        // Simple XOR encryption (use proper encryption in production)
        let key = password.as_bytes();
        let mut encrypted = Vec::new();
        for (i, byte) in json.as_bytes().iter().enumerate() {
            encrypted.push(byte ^ key[i % key.len()]);
        }

        fs::write(output_path, encrypted)
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Import credentials from an encrypted JSON file.
    pub fn import_encrypted(&mut self, password: &str, input_path: &PathBuf) -> Result<(), CredentialError> {
        let encrypted = fs::read(input_path)
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        // Simple XOR decryption
        let key = password.as_bytes();
        let mut decrypted = Vec::new();
        for (i, byte) in encrypted.iter().enumerate() {
            decrypted.push(byte ^ key[i % key.len()]);
        }

        let json = String::from_utf8(decrypted)
            .map_err(|_| CredentialError::InvalidFormat)?;

        let credentials: HashMap<Uuid, Credential> = serde_json::from_str(&json)?;

        for (id, credential) in credentials {
            self.credentials.insert(id, credential.clone());
            self.service_index.insert(credential.service.clone(), id);
        }

        Ok(())
    }

    /// Return the number of stored credentials.
    pub fn count(&self) -> usize {
        self.credentials.len()
    }

    /// Check if a service has credentials stored.
    pub fn has_service(&self, service: &str) -> bool {
        self.service_index.contains_key(service)
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new(SecureStorage::EncryptedFile)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_creation() {
        let credential = Credential::new("github".to_string())
            .with_username("testuser".to_string())
            .with_token("ghp_token".to_string())
            .with_metadata("org".to_string(), "acme".to_string());

        assert_eq!(credential.service, "github");
        assert_eq!(credential.username, Some("testuser".to_string()));
        assert_eq!(credential.token, Some("ghp_token".to_string()));
        assert_eq!(credential.api_key, None);
        assert!(credential.has_secret());
    }

    #[test]
    fn test_credential_manager_store_and_retrieve() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        let credential = Credential::new("github".to_string())
            .with_token("test_token".to_string());

        manager.store(credential).unwrap();

        let retrieved = manager.retrieve("github").unwrap();
        assert_eq!(retrieved.token, Some("test_token".to_string()));
    }

    #[test]
    fn test_credential_manager_replace_service() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        let cred1 = Credential::new("github".to_string())
            .with_token("token1".to_string());
        manager.store(cred1).unwrap();

        let cred2 = Credential::new("github".to_string())
            .with_token("token2".to_string());
        manager.store(cred2).unwrap();

        assert_eq!(manager.count(), 1);
        let retrieved = manager.retrieve("github").unwrap();
        assert_eq!(retrieved.token, Some("token2".to_string()));
    }

    #[test]
    fn test_credential_manager_delete() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        let credential = Credential::new("github".to_string());
        manager.store(credential).unwrap();

        assert!(manager.has_service("github"));
        manager.delete("github").unwrap();
        assert!(!manager.has_service("github"));
    }

    #[test]
    fn test_credential_manager_list_services() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        manager.store(Credential::new("github".to_string())).unwrap();
        manager.store(Credential::new("gitlab".to_string())).unwrap();
        manager.store(Credential::new("openai".to_string())).unwrap();

        let services = manager.list_services();
        assert_eq!(services.len(), 3);
    }

    #[test]
    fn test_credential_manager_not_found() {
        let manager = CredentialManager::new(SecureStorage::Memory);

        let result = manager.retrieve("nonexistent");
        assert!(matches!(result.unwrap_err(), CredentialError::NotFound(_)));
    }

    #[test]
    fn test_credential_serialization() {
        let credential = Credential::new("github".to_string())
            .with_username("testuser".to_string())
            .with_api_key("key123".to_string());

        let json = serde_json::to_string(&credential).unwrap();
        let decoded: Credential = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.service, "github");
        assert_eq!(decoded.username, Some("testuser".to_string()));
        assert_eq!(decoded.api_key, Some("key123".to_string()));
    }

    #[test]
    fn test_credential_touch() {
        let mut credential = Credential::new("test".to_string());
        let original_time = credential.accessed_at;

        // Advance time slightly
        std::thread::sleep(std::time::Duration::from_millis(10));
        credential.touch();

        assert!(credential.accessed_at > original_time);
    }

    #[test]
    fn test_export_import_encrypted() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        let cred = Credential::new("github".to_string())
            .with_token("secret_token".to_string());
        manager.store(cred).unwrap();

        let temp_dir = std::env::temp_dir();
        let export_path = temp_dir.join("test-credentials-export.enc");

        manager.export_encrypted("password123", &export_path).unwrap();

        let mut manager2 = CredentialManager::new(SecureStorage::Memory);
        manager2.import_encrypted("password123", &export_path).unwrap();

        assert_eq!(manager2.count(), 1);
        let retrieved = manager2.retrieve("github").unwrap();
        assert_eq!(retrieved.token, Some("secret_token".to_string()));

        // Cleanup
        let _ = std::fs::remove_file(export_path);
    }

    #[test]
    fn test_export_import_wrong_password() {
        let mut manager = CredentialManager::new(SecureStorage::Memory);

        let cred = Credential::new("github".to_string());
        manager.store(cred).unwrap();

        let temp_dir = std::env::temp_dir();
        let export_path = temp_dir.join("test-credentials-wrong.enc");

        manager.export_encrypted("correct_pass", &export_path).unwrap();

        let mut manager2 = CredentialManager::new(SecureStorage::Memory);
        let result = manager2.import_encrypted("wrong_pass", &export_path);

        // Should fail due to wrong password
        assert!(result.is_err());

        // Cleanup
        let _ = std::fs::remove_file(export_path);
    }

    #[test]
    fn test_credential_has_secret() {
        let mut cred = Credential::new("test".to_string());
        assert!(!cred.has_secret());

        cred = cred.with_token("token".to_string());
        assert!(cred.has_secret());

        let mut cred2 = Credential::new("test2".to_string());
        cred2 = cred2.with_api_key("key".to_string());
        assert!(cred2.has_secret());
    }
}
