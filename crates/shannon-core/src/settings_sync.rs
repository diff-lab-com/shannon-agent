//! # Settings Synchronization
//!
//! Cross-device settings synchronization with conflict resolution and device tracking.
//!
//! ## Architecture
//!
//! This module provides a service for synchronizing settings across multiple devices:
//! - **SettingsSyncService**: Core sync engine with versioned records
//! - **SyncRecord**: Individual setting change with metadata
//! - **DeviceRegistry**: Tracks connected devices and their sync state
//! - **Conflict resolution**: Last-write-wins with version tracking
//!
//! ## Sync Flow
//!
//! 1. Device A modifies a setting -> creates a `SyncRecord` with incremented version
//! 2. Upload records to the sync store
//! 3. Device B downloads pending records and applies changes
//! 4. Conflicts are resolved using last-write-wins (higher version wins)
//!
//! ## Example
//!
//! ```ignore
//! use shannon_core::settings_sync::SettingsSyncService;
//!
//! let mut sync = SettingsSyncService::new("device-abc");
//! sync.upsert("model", "claude-opus-4-6");
//! sync.upsert("temperature", "0.7");
//!
//! let pending = sync.pending_uploads();
//! assert_eq!(pending.len(), 2);
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during settings synchronization.
#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device already registered: {0}")]
    DeviceAlreadyExists(String),

    #[error("Conflict detected for key '{key}': local version {local}, remote version {remote}")]
    Conflict {
        key: String,
        local: u64,
        remote: u64,
    },

    #[error("Record not found: {0}")]
    RecordNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid device ID: {0}")]
    InvalidDeviceId(String),
}

// ============================================================================
// Core Types
// ============================================================================

/// Current synchronization status for a setting key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum SyncStatus {
    /// Local and remote are in sync.
    UpToDate,
    /// Local has changes that need to be uploaded.
    PendingUpload,
    /// Remote has changes that need to be downloaded.
    PendingDownload,
    /// Both local and remote have changed since last sync.
    Conflict,
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncStatus::UpToDate => write!(f, "up_to_date"),
            SyncStatus::PendingUpload => write!(f, "pending_upload"),
            SyncStatus::PendingDownload => write!(f, "pending_download"),
            SyncStatus::Conflict => write!(f, "conflict"),
        }
    }
}

/// A single versioned settings record for synchronization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncRecord {
    /// The setting key (e.g., "model", "temperature").
    pub key: String,
    /// The setting value as a string.
    pub value: String,
    /// When this record was last modified.
    pub timestamp: DateTime<Utc>,
    /// ID of the device that last modified this record.
    pub device_id: String,
    /// Monotonically increasing version number for conflict detection.
    pub version: u64,
}

impl SyncRecord {
    /// Create a new sync record.
    pub fn new(key: &str, value: &str, device_id: &str, version: u64) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            timestamp: Utc::now(),
            device_id: device_id.to_string(),
            version,
        }
    }
}

/// Information about a registered device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceInfo {
    /// Unique identifier for this device.
    pub device_id: String,
    /// Human-readable device name.
    pub device_name: String,
    /// When this device was first registered.
    pub registered_at: DateTime<Utc>,
    /// When this device was last seen (last sync).
    pub last_seen: DateTime<Utc>,
    /// The highest version number this device has acknowledged.
    pub last_synced_version: u64,
}

impl DeviceInfo {
    /// Create a new device info entry.
    pub fn new(device_id: &str, device_name: &str) -> Self {
        let now = Utc::now();
        Self {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            registered_at: now,
            last_seen: now,
            last_synced_version: 0,
        }
    }
}

// ============================================================================
// Device Registry
// ============================================================================

/// Registry for tracking devices participating in settings synchronization.
pub struct DeviceRegistry {
    /// Registered devices keyed by device ID.
    devices: HashMap<String, DeviceInfo>,
}

impl DeviceRegistry {
    /// Create an empty device registry.
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    /// Register a new device.
    pub fn register(
        &mut self,
        device_id: &str,
        device_name: &str,
    ) -> Result<&DeviceInfo, SyncError> {
        if self.devices.contains_key(device_id) {
            return Err(SyncError::DeviceAlreadyExists(device_id.to_string()));
        }

        let info = DeviceInfo::new(device_id, device_name);
        self.devices.insert(device_id.to_string(), info);
        Ok(self
            .devices
            .get(device_id)
            .expect("device was just inserted after contains_key check"))
    }

    /// Unregister a device.
    pub fn unregister(&mut self, device_id: &str) -> Result<(), SyncError> {
        self.devices
            .remove(device_id)
            .map(|_| ())
            .ok_or_else(|| SyncError::DeviceNotFound(device_id.to_string()))
    }

    /// Get a device by ID.
    pub fn get(&self, device_id: &str) -> Result<&DeviceInfo, SyncError> {
        self.devices
            .get(device_id)
            .ok_or_else(|| SyncError::DeviceNotFound(device_id.to_string()))
    }

    /// Update the last-seen timestamp and synced version for a device.
    pub fn heartbeat(&mut self, device_id: &str, synced_version: u64) -> Result<(), SyncError> {
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| SyncError::DeviceNotFound(device_id.to_string()))?;

        device.last_seen = Utc::now();
        if synced_version > device.last_synced_version {
            device.last_synced_version = synced_version;
        }
        Ok(())
    }

    /// List all registered device IDs.
    pub fn list_device_ids(&self) -> Vec<&String> {
        self.devices.keys().collect()
    }

    /// List all registered devices.
    pub fn list_devices(&self) -> Vec<&DeviceInfo> {
        self.devices.values().collect()
    }

    /// Return the number of registered devices.
    pub fn count(&self) -> usize {
        self.devices.len()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Settings Sync Service
// ============================================================================

/// Core service for synchronizing settings across devices.
///
/// Uses a last-write-wins strategy with version tracking for conflict resolution.
/// Each setting is stored as a versioned record, and conflicts are resolved
/// by keeping the record with the highest version number.
pub struct SettingsSyncService {
    /// The local device ID.
    device_id: String,
    /// Versioned settings records keyed by setting key.
    records: HashMap<String, SyncRecord>,
    /// Tracks which records have been modified locally since last upload.
    dirty_keys: HashMap<String, bool>,
    /// Tracks which records have been received remotely but not yet applied.
    pending_downloads: Vec<SyncRecord>,
    /// Global version counter (monotonically increasing).
    global_version: u64,
    /// Registry of participating devices.
    device_registry: DeviceRegistry,
}

impl SettingsSyncService {
    /// Create a new sync service for the given device.
    pub fn new(device_id: &str) -> Self {
        let mut service = Self {
            device_id: device_id.to_string(),
            records: HashMap::new(),
            dirty_keys: HashMap::new(),
            pending_downloads: Vec::new(),
            global_version: 0,
            device_registry: DeviceRegistry::new(),
        };

        // Register the local device
        service
            .device_registry
            .register(device_id, &format!("Device {device_id}"))
            .ok(); // Ignore error if already registered (shouldn't happen for new service)

        service
    }

    /// Create a sync service with a pre-populated device registry.
    pub fn with_devices(device_id: &str, devices: Vec<(&str, &str)>) -> Self {
        let mut service = Self::new(device_id);

        for (id, name) in devices {
            service.device_registry.register(id, name).ok(); // Ignore duplicates
        }

        service
    }

    // -----------------------------------------------------------------------
    // Local Operations
    // -----------------------------------------------------------------------

    /// Insert or update a setting locally, marking it for upload.
    ///
    /// If the key already exists, the version is incremented.
    /// If the key is new, it starts at the next global version.
    pub fn upsert(&mut self, key: &str, value: &str) {
        self.global_version += 1;

        let version = match self.records.get(key) {
            Some(existing) => existing.version + 1,
            None => self.global_version,
        };

        // Ensure the global version is always at least as high as any record version
        if version > self.global_version {
            self.global_version = version;
        }

        let record = SyncRecord::new(key, value, &self.device_id, version);
        self.records.insert(key.to_string(), record);
        self.dirty_keys.insert(key.to_string(), true);
    }

    /// Get the current local value for a setting key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.records.get(key).map(|r| r.value.as_str())
    }

    /// Remove a setting locally, marking it for upload.
    pub fn remove(&mut self, key: &str) -> Result<(), SyncError> {
        if self.records.remove(key).is_none() {
            return Err(SyncError::RecordNotFound(key.to_string()));
        }
        self.dirty_keys.remove(key);
        Ok(())
    }

    /// Get all local setting keys.
    pub fn keys(&self) -> Vec<&String> {
        self.records.keys().collect()
    }

    // -----------------------------------------------------------------------
    // Sync Operations
    // -----------------------------------------------------------------------

    /// Get records that need to be uploaded to the remote.
    ///
    /// Returns records marked as dirty. Call [`clear_pending_uploads`](Self::clear_pending_uploads)
    /// after a successful upload.
    pub fn pending_uploads(&self) -> Vec<&SyncRecord> {
        self.dirty_keys
            .keys()
            .filter_map(|k| self.records.get(k))
            .collect()
    }

    /// Clear the dirty flag for all keys (after successful upload).
    pub fn clear_pending_uploads(&mut self) {
        self.dirty_keys.clear();
    }

    /// Clear the dirty flag for a specific key.
    pub fn mark_uploaded(&mut self, key: &str) {
        self.dirty_keys.remove(key);
    }

    /// Get records pending download (received from remote but not yet applied).
    pub fn pending_downloads(&self) -> &[SyncRecord] {
        &self.pending_downloads
    }

    /// Receive records from a remote source (simulating a download).
    ///
    /// Each incoming record is compared against the local version:
    /// - If the remote version is higher, it is queued for download
    /// - If the local version is higher and dirty, a conflict is flagged
    /// - If versions match, no action is needed
    pub fn receive_remote_records(
        &mut self,
        remote_records: Vec<SyncRecord>,
    ) -> Vec<(String, SyncStatus)> {
        let mut statuses = Vec::new();

        for remote in remote_records {
            let key = remote.key.clone();

            match self.records.get(&key) {
                None => {
                    // New record from remote
                    statuses.push((key.clone(), SyncStatus::PendingDownload));
                    self.pending_downloads.push(remote);
                }
                Some(local) => {
                    if remote.version > local.version {
                        if self.dirty_keys.contains_key(&key) {
                            // Both modified: conflict
                            statuses.push((key.clone(), SyncStatus::Conflict));
                            self.pending_downloads.push(remote);
                        } else {
                            // Remote is newer and local is clean
                            statuses.push((key.clone(), SyncStatus::PendingDownload));
                            self.pending_downloads.push(remote);
                        }
                    } else if remote.version == local.version {
                        statuses.push((key.clone(), SyncStatus::UpToDate));
                    } else {
                        // Local is newer
                        if self.dirty_keys.contains_key(&key) {
                            statuses.push((key.clone(), SyncStatus::PendingUpload));
                        } else {
                            statuses.push((key.clone(), SyncStatus::UpToDate));
                        }
                    }
                }
            }
        }

        statuses
    }

    /// Apply all pending downloads, resolving conflicts using last-write-wins.
    ///
    /// For conflicts, the remote record wins (higher version).
    /// After applying, the dirty flag is cleared for those keys.
    pub fn apply_pending_downloads(&mut self) -> Vec<SyncRecord> {
        let applied: Vec<SyncRecord> = self.pending_downloads.drain(..).collect();

        for record in &applied {
            match self.records.get(&record.key) {
                None => {
                    // Pure new record
                    self.records.insert(record.key.clone(), record.clone());
                }
                Some(local) => {
                    if record.version >= local.version {
                        // Remote wins (last-write-wins)
                        self.records.insert(record.key.clone(), record.clone());
                        self.dirty_keys.remove(&record.key);
                    }
                    // If local version is higher, keep local (shouldn't normally happen
                    // after conflict resolution, but we preserve it as a safety measure)
                }
            }

            // Update global version
            if record.version > self.global_version {
                self.global_version = record.version;
            }
        }

        applied
    }

    /// Check the sync status for a specific key.
    pub fn sync_status(&self, key: &str) -> SyncStatus {
        if self.dirty_keys.contains_key(key) {
            if self.pending_downloads.iter().any(|r| r.key == key) {
                return SyncStatus::Conflict;
            }
            return SyncStatus::PendingUpload;
        }

        if self.pending_downloads.iter().any(|r| r.key == key) {
            return SyncStatus::PendingDownload;
        }

        SyncStatus::UpToDate
    }

    /// Get sync status for all tracked keys.
    pub fn all_sync_statuses(&self) -> HashMap<String, SyncStatus> {
        let mut statuses = HashMap::new();

        for key in self.records.keys() {
            statuses.insert(key.clone(), self.sync_status(key));
        }

        // Include pending download keys not yet in records
        for record in &self.pending_downloads {
            if !statuses.contains_key(&record.key) {
                statuses.insert(record.key.clone(), SyncStatus::PendingDownload);
            }
        }

        statuses
    }

    // -----------------------------------------------------------------------
    // Device Registry Access
    // -----------------------------------------------------------------------

    /// Get a reference to the device registry.
    pub fn device_registry(&self) -> &DeviceRegistry {
        &self.device_registry
    }

    /// Get a mutable reference to the device registry.
    pub fn device_registry_mut(&mut self) -> &mut DeviceRegistry {
        &mut self.device_registry
    }

    /// Get the local device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Get the current global version counter.
    pub fn global_version(&self) -> u64 {
        self.global_version
    }

    /// Get the number of records tracked.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Get the number of pending downloads.
    pub fn pending_download_count(&self) -> usize {
        self.pending_downloads.len()
    }

    /// Get the number of pending uploads.
    pub fn pending_upload_count(&self) -> usize {
        self.dirty_keys.len()
    }

    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    /// Export all records as a JSON string for persistence or transport.
    pub fn export_records(&self) -> Result<String, SyncError> {
        let records: Vec<&SyncRecord> = self.records.values().collect();
        serde_json::to_string(&records).map_err(|e| SyncError::Serialization(e.to_string()))
    }

    /// Import records from a JSON string, merging with existing records.
    ///
    /// Uses last-write-wins: higher version always wins.
    pub fn import_records(&mut self, json: &str) -> Result<Vec<SyncStatus>, SyncError> {
        let incoming: Vec<SyncRecord> =
            serde_json::from_str(json).map_err(|e| SyncError::Serialization(e.to_string()))?;

        let statuses = self.receive_remote_records(incoming);
        Ok(statuses.into_iter().map(|(_, s)| s).collect())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sync_service() {
        let service = SettingsSyncService::new("device-1");
        assert_eq!(service.device_id(), "device-1");
        assert_eq!(service.record_count(), 0);
        assert_eq!(service.device_registry().count(), 1);
    }

    #[test]
    fn test_upsert_and_get() {
        let mut service = SettingsSyncService::new("device-1");

        service.upsert("model", "claude-opus-4-6");
        assert_eq!(service.get("model"), Some("claude-opus-4-6"));
        assert_eq!(service.record_count(), 1);
        assert_eq!(service.pending_upload_count(), 1);

        // Update existing key
        service.upsert("model", "claude-sonnet-4-6");
        assert_eq!(service.get("model"), Some("claude-sonnet-4-6"));

        // Non-existent key
        assert_eq!(service.get("nonexistent"), None);
    }

    #[test]
    fn test_remove_key() {
        let mut service = SettingsSyncService::new("device-1");
        service.upsert("model", "claude-opus-4-6");

        assert!(service.remove("model").is_ok());
        assert_eq!(service.get("model"), None);
        assert_eq!(service.record_count(), 0);

        // Remove non-existent key
        assert!(service.remove("nonexistent").is_err());
    }

    #[test]
    fn test_pending_uploads() {
        let mut service = SettingsSyncService::new("device-1");

        service.upsert("model", "claude-opus-4-6");
        service.upsert("temperature", "0.7");

        let pending = service.pending_uploads();
        assert_eq!(pending.len(), 2);

        // Clear uploads
        service.clear_pending_uploads();
        assert_eq!(service.pending_upload_count(), 0);

        // New upsert should mark dirty again
        service.upsert("theme", "dark");
        assert_eq!(service.pending_upload_count(), 1);
    }

    #[test]
    fn test_receive_remote_new_records() {
        let mut service = SettingsSyncService::new("device-1");

        let remote_records = vec![
            SyncRecord::new("model", "claude-opus-4-6", "device-2", 1),
            SyncRecord::new("theme", "light", "device-2", 2),
        ];

        let statuses = service.receive_remote_records(remote_records);

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].1, SyncStatus::PendingDownload);
        assert_eq!(statuses[1].1, SyncStatus::PendingDownload);
        assert_eq!(service.pending_download_count(), 2);
    }

    #[test]
    fn test_receive_remote_conflict() {
        let mut service = SettingsSyncService::new("device-1");

        // Local modification (dirty)
        service.upsert("model", "claude-opus-4-6");

        // Remote has a newer version of the same key
        let local_version = service.records.get("model").unwrap().version;
        let remote_records = vec![SyncRecord::new(
            "model",
            "claude-sonnet-4-6",
            "device-2",
            local_version + 1,
        )];

        let statuses = service.receive_remote_records(remote_records);

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].1, SyncStatus::Conflict);
    }

    #[test]
    fn test_apply_pending_downloads() {
        let mut service = SettingsSyncService::new("device-1");

        // Manually inject pending downloads
        service
            .pending_downloads
            .push(SyncRecord::new("model", "claude-opus-4-6", "device-2", 5));
        service
            .pending_downloads
            .push(SyncRecord::new("theme", "light", "device-2", 6));

        let applied = service.apply_pending_downloads();

        assert_eq!(applied.len(), 2);
        assert_eq!(service.get("model"), Some("claude-opus-4-6"));
        assert_eq!(service.get("theme"), Some("light"));
        assert_eq!(service.pending_download_count(), 0);
    }

    #[test]
    fn test_sync_status_tracking() {
        let mut service = SettingsSyncService::new("device-1");

        // No records -> UpToDate by default for everything
        assert_eq!(service.sync_status("model"), SyncStatus::UpToDate);

        // Local modification -> PendingUpload
        service.upsert("model", "claude-opus-4-6");
        assert_eq!(service.sync_status("model"), SyncStatus::PendingUpload);

        // After upload
        service.clear_pending_uploads();
        assert_eq!(service.sync_status("model"), SyncStatus::UpToDate);

        // Remote record pending
        service
            .pending_downloads
            .push(SyncRecord::new("theme", "light", "device-2", 1));
        assert_eq!(service.sync_status("theme"), SyncStatus::PendingDownload);
    }

    #[test]
    fn test_device_registry_operations() {
        let mut registry = DeviceRegistry::new();

        registry.register("dev-1", "Laptop").unwrap();
        registry.register("dev-2", "Desktop").unwrap();

        assert_eq!(registry.count(), 2);

        let dev = registry.get("dev-1").unwrap();
        assert_eq!(dev.device_name, "Laptop");

        // Duplicate registration
        assert!(registry.register("dev-1", "Another").is_err());

        // Heartbeat
        registry.heartbeat("dev-1", 42).unwrap();
        assert_eq!(registry.get("dev-1").unwrap().last_synced_version, 42);

        // Unregister
        registry.unregister("dev-1").unwrap();
        assert_eq!(registry.count(), 1);

        // Non-existent
        assert!(registry.get("dev-1").is_err());
        assert!(registry.unregister("dev-1").is_err());
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut service = SettingsSyncService::new("device-1");
        service.upsert("model", "claude-opus-4-6");
        service.upsert("temperature", "0.7");

        let exported = service.export_records().unwrap();

        let mut other_service = SettingsSyncService::new("device-2");
        let statuses = other_service.import_records(&exported).unwrap();

        // All should be PendingDownload since they're new to device-2
        assert!(statuses.iter().all(|s| *s == SyncStatus::PendingDownload));
        assert_eq!(other_service.pending_download_count(), 2);

        other_service.apply_pending_downloads();
        assert_eq!(other_service.get("model"), Some("claude-opus-4-6"));
        assert_eq!(other_service.get("temperature"), Some("0.7"));
    }

    #[test]
    fn test_sync_record_serialization() {
        let record = SyncRecord::new("model", "claude-opus-4-6", "device-1", 42);

        let json = serde_json::to_string(&record).unwrap();
        let deserialized: SyncRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(record, deserialized);
    }

    #[test]
    fn test_device_info_serialization() {
        let info = DeviceInfo::new("dev-1", "Laptop");

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: DeviceInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_all_sync_statuses() {
        let mut service = SettingsSyncService::new("device-1");

        service.upsert("model", "claude-opus-4-6");
        service
            .pending_downloads
            .push(SyncRecord::new("theme", "light", "device-2", 1));

        let statuses = service.all_sync_statuses();
        assert_eq!(statuses.get("model"), Some(&SyncStatus::PendingUpload));
        assert_eq!(statuses.get("theme"), Some(&SyncStatus::PendingDownload));
    }

    #[test]
    fn test_version_increments_on_upsert() {
        let mut service = SettingsSyncService::new("device-1");

        service.upsert("key", "v1");
        let v1 = service.records.get("key").unwrap().version;

        service.upsert("key", "v2");
        let v2 = service.records.get("key").unwrap().version;

        assert!(v2 > v1);
    }

    #[test]
    fn test_with_devices_constructor() {
        let service = SettingsSyncService::with_devices(
            "dev-1",
            vec![("dev-2", "Desktop"), ("dev-3", "Phone")],
        );

        assert_eq!(service.device_registry().count(), 3);
        assert!(service.device_registry().get("dev-2").is_ok());
        assert!(service.device_registry().get("dev-3").is_ok());
    }
}
