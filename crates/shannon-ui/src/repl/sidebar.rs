//! Sidebar info, context pressure, agent refresh, and auto-compaction.

use crate::widgets::ChatRole;
use super::state::AgentDisplay;

/// Read process RSS memory in KB from /proc/self/status (Linux).
fn read_memory_rss_kb() -> u64 {
    let Ok(data) = std::fs::read_to_string("/proc/self/status") else { return 0 };
    for line in data.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }
    0
}

impl super::Repl {
    /// Get the current REPL state
    pub fn state(&self) -> &super::ReplState {
        &self.state
    }

    /// Get mutable reference to the REPL state
    pub fn state_mut(&mut self) -> &mut super::ReplState {
        &mut self.state
    }

    /// Build sidebar info from the current state, if the sidebar is visible.
    pub fn sidebar_info(&self) -> Option<crate::widgets::SidebarInfo> {
        if !self.state.sidebar_visible {
            return None;
        }
        let mut modified_files: Vec<(String, usize, usize)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for turn in self.diff_data.get_session_diffs() {
            for fc in &turn.files_modified {
                if seen.insert(fc.path.clone()) {
                    modified_files.push((fc.path.clone(), fc.additions, fc.deletions));
                }
            }
        }
        let error_count = self.chat.iter_messages()
            .filter(|(_, m)| m.role == ChatRole::Tool && m.is_error)
            .count();
        let context_window = self.state.model.as_deref()
            .map(shannon_core::model_registry::context_window_for)
            .unwrap_or(200_000);

        // Refresh active_agents from registry if available
        let active_agents = if self.agent_registry.is_some() {
            // We can't easily call async .list() from this sync method,
            // so use the cached state.active_agents which is refreshed
            // in the main loop after coordinator events.
            self.state.active_agents.clone()
        } else {
            Vec::new()
        };

        let diagnostics: Vec<_> = self.state.diagnostic_store.diagnostics.iter().take(50).map(|d| crate::lsp_bridge::Diagnostic {
            severity: d.severity,
            message: d.message.clone(),
            file_path: d.file_path.clone(),
            line: d.line,
            source: d.source.clone(),
        }).collect();

        Some(crate::widgets::SidebarInfo {
            model: self.state.model.clone(),
            tokens_used: self.state.tokens_used,
            cost_usd: self.state.total_cost_usd,
            tools_invoked: self.tools_invoked,
            modified_files,
            total_additions: self.diff_data.total_additions(),
            total_deletions: self.diff_data.total_deletions(),
            error_count,
            context_window,
            active_agents,
            diagnostics,
            session_duration_secs: self.state.session_start
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            turn_count: self.current_turn,
            commands_run: self.commands_run,
            tokens_per_sec: {
                let dur = self.state.session_start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
                if dur > 0.0 && self.state.tokens_used > 0 {
                    Some(self.state.tokens_used as f64 / dur)
                } else {
                    None
                }
            },
            memory_rss_kb: read_memory_rss_kb(),
        })
    }

    /// Check context pressure and auto-compact if needed.
    /// Returns true if auto-compaction was performed.
    pub fn check_context_pressure(&mut self) -> bool {
        let context_window = self.state.model.as_deref()
            .map(shannon_core::model_registry::context_window_for)
            .unwrap_or(200_000) as u64;

        if context_window == 0 || self.state.tokens_used == 0 {
            return false;
        }

        let usage_ratio = self.state.tokens_used as f64 / context_window as f64;

        if usage_ratio > 0.85 {
            // Auto-compact: context pressure critical (>85%)
            self.do_auto_compact();
            return true;
        } else if usage_ratio > 0.70 {
            // Warning: context pressure high (>70%)
            let pct = (usage_ratio * 100.0) as u32;
            let remaining = context_window.saturating_sub(self.state.tokens_used);
            self.state.toast = Some((
                format!("  Context: {pct}% used ({remaining} tokens remaining) — /compact to reduce  "),
                std::time::Instant::now(),
            ));
        }
        false
    }

    /// Refresh active_agents from the SubAgentRegistry for sidebar display.
    /// Called from the main loop tick; uses the tokio runtime for async access.
    /// Detects agent completions and sends desktop notifications.
    pub fn refresh_agents(&mut self) {
        if let Some(ref registry) = self.agent_registry {
            let agents = self.runtime.block_on(registry.list_agents());

            // Detect agents that transitioned from active to completed/failed
            let prev_names: std::collections::HashSet<String> = self.state.active_agents
                .iter()
                .filter(|a| a.active)
                .map(|a| a.name.clone())
                .collect();

            let new_agents: Vec<AgentDisplay> = agents.into_iter().map(|a| {
                let active = matches!(a.status, shannon_agents::AgentStatus::Running | shannon_agents::AgentStatus::Spawning | shannon_agents::AgentStatus::Idle);
                AgentDisplay {
                    name: a.name,
                    status: a.status.to_string(),
                    active,
                    team: a.team,
                    turns_used: a.turns_used,
                    max_turns: a.config.max_turns,
                }
            }).collect();

            // Send desktop notification for newly completed agents
            use shannon_core::notifier::{DesktopNotifier, NotificationHandler, Notification, NotificationLevel};
            use chrono::Utc;

            for agent in &new_agents {
                if !agent.active && prev_names.contains(&agent.name) {
                    let notifier = DesktopNotifier::new();
                    let status = &agent.status;
                    if status == "completed" {
                        let notification = Notification {
                            title: format!("Agent {} completed", agent.name),
                            body: format!("Finished after {} turns", agent.turns_used),
                            level: NotificationLevel::Success,
                            id: format!("agent-{}-done", agent.name),
                            timestamp: Utc::now(),
                        };
                        let _ = notifier.send(&notification);
                    } else if status.starts_with("failed") {
                        let notification = Notification {
                            title: format!("Agent {} failed", agent.name),
                            body: status.clone(),
                            level: NotificationLevel::Error,
                            id: format!("agent-{}-fail", agent.name),
                            timestamp: Utc::now(),
                        };
                        let _ = notifier.send(&notification);
                    }
                }
            }

            self.state.active_agents = new_agents;
        }
    }

    /// Perform auto-compaction using LLM-powered summarization.
    pub(crate) fn do_auto_compact(&mut self) {
        use shannon_core::compact::CompactEngine;

        let Some(ref mut engine) = self.query_engine else { return };

        let history = engine.conversation_history();
        if history.len() < 4 {
            return; // Not enough to compact
        }

        // Use LLM summarizer for higher quality compression, fallback to rule-based
        let client = engine.client().clone();
        let compact_engine = match CompactEngine::with_llm_summarizer(client) {
            Ok(e) => e,
            Err(_) => match CompactEngine::with_defaults() {
                Ok(e) => e,
                Err(_) => return,
            },
        };

        let before = history.len();
        let mut messages = history;

        // Use truncate strategy for auto-compact — fast, no extra API call
        if let Ok(result) = compact_engine.micro_compact(&mut messages) {
            let _ = compact_engine.post_compact_cleanup(&mut messages);
            engine.replace_conversation(messages);

            let after = engine.conversation_history().len();
            self.state.toast = Some((
                format!("  Auto-compacted: {before}→{after} messages  "),
                std::time::Instant::now(),
            ));
            tracing::info!("Auto-compacted context: {before}→{after} messages, {:.0}% reduction",
                result.reduction_ratio * 100.0);
        }
    }
}
