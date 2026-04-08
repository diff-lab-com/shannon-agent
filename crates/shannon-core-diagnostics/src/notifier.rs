//! Notification system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Notification priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Urgent,
}

/// Notification channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    pub id: Uuid,
    pub name: String,
    pub channel_type: ChannelType,
    pub enabled: bool,
    pub config: HashMap<String, String>,
}

/// Channel type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChannelType {
    Desktop,
    Email,
    Webhook,
    Slack,
    Discord,
    Custom(String),
}

/// Notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub channel_id: Uuid,
    pub metadata: HashMap<String, String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub read: bool,
}

impl Notification {
    pub fn new(title: String, body: String, priority: NotificationPriority, channel_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            title,
            body,
            priority,
            channel_id,
            metadata: HashMap::new(),
            timestamp: chrono::Utc::now(),
            read: false,
        }
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn mark_as_read(&mut self) {
        self.read = true;
    }
}

/// Notifier
pub struct Notifier {
    channels: Vec<NotificationChannel>,
    notifications: Vec<Notification>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            notifications: Vec::new(),
        }
    }

    /// Add a channel
    pub fn add_channel(&mut self, channel: NotificationChannel) {
        self.channels.push(channel);
    }

    /// Remove a channel
    pub fn remove_channel(&mut self, id: &Uuid) {
        self.channels.retain(|c| c.id != *id);
    }

    /// Send a notification
    pub async fn send(&mut self, mut notification: Notification) -> Result<(), NotifyError> {
        // Find the channel
        let channel = self
            .channels
            .iter()
            .find(|c| c.id == notification.channel_id)
            .ok_or_else(|| NotifyError::ChannelNotFound(notification.channel_id))?;

        if !channel.enabled {
            return Err(NotifyError::ChannelDisabled(channel.name.clone()));
        }

        // In a real implementation, this would send to the actual channel
        // For now, we just store it
        self.notifications.push(notification.clone());

        tracing::info!(
            "Notification sent via {}: {}",
            channel.name,
            notification.title
        );

        Ok(())
    }

    /// Get all notifications
    pub fn get_notifications(&self) -> &[Notification] {
        &self.notifications
    }

    /// Get unread notifications
    pub fn get_unread(&self) -> Vec<&Notification> {
        self.notifications.iter().filter(|n| !n.read).collect()
    }

    /// Mark notification as read
    pub fn mark_as_read(&mut self, id: &Uuid) -> Result<(), NotifyError> {
        let notification = self
            .notifications
            .iter_mut()
            .find(|n| n.id == *id)
            .ok_or_else(|| NotifyError::NotificationNotFound(*id))?;

        notification.mark_as_read();
        Ok(())
    }

    /// Clear all notifications
    pub fn clear(&mut self) {
        self.notifications.clear();
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Notification errors
#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("Channel not found: {0}")]
    ChannelNotFound(Uuid),

    #[error("Channel disabled: {0}")]
    ChannelDisabled(String),

    #[error("Notification not found: {0}")]
    NotificationNotFound(Uuid),

    #[error("Send failed: {0}")]
    SendFailed(String),
}
