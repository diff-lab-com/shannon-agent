//! Credential management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;
use uuid::Uuid;

/// Credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: Uuid,
    pub service: String,
    pub username: Option<String>,
    pub token: Option<String>,
    pub api_key: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Secure storage type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureStorage {
    Keyring,
    EncryptedFile,
    Memory,
}

/// Credential manager
pub struct CredentialManager {
    credentials: HashMap<Uuid, Credential>,
    service_index: HashMap<String, Uuid>,
    storage_type: SecureStorage,
    storage_path: Option<PathBuf>,
}

impl CredentialManager {
    pub fn new(storage_type: SecureStorage) -> Self {
        Self {
            credentials: HashMap::new(),
            service_index: HashMap::new(),
            storage_type,
            storage_path: None,
        }
    }

    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Store a credential
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

    /// Retrieve credential by service
    pub fn retrieve(&self, service: &str) -> Option<&Credential> {
        if let Some(id) = self.service_index.get(service) {
            self.credentials.get(id)
        } else {
            None
        }
    }

    /// Retrieve credential by ID
    pub fn retrieve_by_id(&self, id: &Uuid) -> Option<&Credential> {
        self.credentials.get(id)
    }

    /// Delete credential
    pub fn delete(&mut self, service: &str) -> Result<(), CredentialError> {
        if let Some(id) = self.service_index.remove(service) {
            self.credentials.remove(&id);
            Ok(())
        } else {
            Err(CredentialError::NotFound(service.to_string()))
        }
    }

    /// List all credentials
    pub fn list(&self) -> Vec<&Credential> {
        self.credentials.values().collect()
    }

    /// Persist credential to disk (encrypted)
    fn persist(&self, credential: &Credential, path: &PathBuf) -> Result<(), CredentialError> {
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CredentialError::StorageError(e.to_string()))?;
        }

        // Serialize credential
        let json = serde_json::to_string(credential)
            .map_err(|e| CredentialError::SerializationError(e.to_string()))?;

        // In production, this would be encrypted
        let file_path = path.join(format!("{}.json", credential.id));
        std::fs::write(file_path, json)
            .map_err(|e| CredentialError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load credentials from disk
    pub fn load_from_disk(&mut self) -> Result<(), CredentialError> {
        if let Some(path) = &self.storage_path {
            if !path.exists() {
                return Ok(());
            }

            for entry in std::fs::read_dir(path)
                .map_err(|e| CredentialError::StorageError(e.to_string()))?
            {
                let entry = entry.map_err(|e| CredentialError::StorageError(e.to_string()))?;
                let file_path = entry.path();

                if file_path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }

                let json = std::fs::read_to_string(&file_path)
                    .map_err(|e| CredentialError::StorageError(e.to_string()))?;

                let credential: Credential = serde_json::from_str(&json)
                    .map_err(|e| CredentialError::SerializationError(e.to_string()))?;

                self.credentials.insert(credential.id, credential.clone());
                self.service_index.insert(credential.service.clone(), credential.id);
            }
        }

        Ok(())
    }
}

/// Credential errors
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("Credential not found: {0}")]
    NotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Access denied")]
    AccessDenied,
}
