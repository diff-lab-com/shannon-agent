//! Tmux integration for displaying agent activity in split panes.

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages tmux split panes for displaying agent output.
///
/// Each agent can have its own tmux pane where its activity is shown.
/// This provides a real-time view of what each agent is doing.
#[derive(Debug, Clone)]
pub struct TmuxManager {
    /// Whether tmux is available and we're in a tmux session
    available: bool,
    /// Map of agent name to tmux pane ID
    panes: Arc<RwLock<HashMap<String, String>>>,
}

impl TmuxManager {
    /// Create a new TmuxManager, detecting tmux availability.
    pub fn new() -> Self {
        let available = Self::detect_tmux();
        if available {
            tracing::info!("Tmux detected — agent panes will be available");
        } else {
            tracing::debug!("Tmux not available or not in a tmux session");
        }
        Self {
            available,
            panes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if tmux is available and we're in a tmux session.
    fn detect_tmux() -> bool {
        // Check TMUX env var (set when inside a tmux session)
        if std::env::var("TMUX").is_err() {
            return false;
        }
        // Check that tmux binary exists
        Command::new("tmux")
            .arg("list-panes")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Whether tmux panes are available for use.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Create a split pane for an agent.
    ///
    /// Returns the pane ID on success, or an error message on failure.
    pub async fn create_agent_pane(&self, agent_name: &str) -> Result<String, String> {
        if !self.available {
            return Err("Tmux not available".to_string());
        }

        // Split the current window horizontally
        let output = Command::new("tmux")
            .args(["split-window", "-h", "-P", "-F", "#{pane_id}"])
            .env("TMUX_PANE", "")
            .output()
            .map_err(|e| format!("Failed to run tmux: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tmux split-window failed: {stderr}"));
        }

        let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if pane_id.is_empty() {
            return Err("tmux returned empty pane ID".to_string());
        }

        // Set the pane title to the agent name
        let _ = Command::new("tmux")
            .args(["select-pane", "-T", &format!("Agent: {agent_name}")])
            .output();

        // Send a header to the pane
        self.send_to_pane(&pane_id, &format!("--- Agent: {agent_name} ---\n"))
            .await;

        self.panes
            .write()
            .await
            .insert(agent_name.to_string(), pane_id.clone());

        tracing::info!(
            agent = %agent_name,
            pane = %pane_id,
            "Created tmux pane for agent"
        );

        Ok(pane_id)
    }

    /// Send output text to an agent's pane.
    pub async fn send_output(&self, agent_name: &str, text: &str) {
        let panes = self.panes.read().await;
        if let Some(pane_id) = panes.get(agent_name) {
            self.send_to_pane(pane_id, text).await;
        }
    }

    /// Send text to a specific tmux pane.
    async fn send_to_pane(&self, pane_id: &str, text: &str) {
        // Use tmux send-keys to send text to the pane
        // The 'Enter' at the end just makes the text visible
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "-l", text])
            .output();

        // Add a newline for readability
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "Enter"])
            .output();
    }

    /// Remove an agent's pane.
    pub async fn remove_agent_pane(&self, agent_name: &str) {
        let pane_id = self.panes.write().await.remove(agent_name);
        if let Some(pane_id) = pane_id {
            // Kill the pane
            let _ = Command::new("tmux")
                .args(["kill-pane", "-t", &pane_id])
                .output();

            tracing::info!(
                agent = %agent_name,
                pane = %pane_id,
                "Removed tmux pane for agent"
            );
        }
    }

    /// List all active agent panes.
    pub async fn active_panes(&self) -> Vec<(String, String)> {
        self.panes
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Remove all agent panes (cleanup).
    pub async fn remove_all(&self) {
        let panes = self.panes.write().await;
        for (agent_name, pane_id) in panes.iter() {
            let _ = Command::new("tmux")
                .args(["kill-pane", "-t", pane_id])
                .output();

            tracing::debug!(
                agent = %agent_name,
                pane = %pane_id,
                "Cleaned up tmux pane"
            );
        }
    }
}

impl Default for TmuxManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_tmux_manager_default() {
        let mgr = TmuxManager::default();
        // In test environments, tmux is typically not available
        // so we just verify construction works
        let _ = &mgr.available;
    }

    #[tokio::test]
    async fn test_send_output_no_pane() {
        let mgr = TmuxManager::new();
        // Should not panic when sending to non-existent pane
        mgr.send_output("nonexistent", "test output").await;
    }

    #[tokio::test]
    async fn test_remove_nonexistent_pane() {
        let mgr = TmuxManager::new();
        // Should not panic when removing non-existent pane
        mgr.remove_agent_pane("nonexistent").await;
    }

    #[tokio::test]
    async fn test_active_panes_empty() {
        let mgr = TmuxManager::new();
        let panes = mgr.active_panes().await;
        assert!(panes.is_empty());
    }
}
