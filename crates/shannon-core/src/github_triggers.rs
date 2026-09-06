//! GitHub event triggers for routines (P2-7).
//!
//! A [`ScheduledRoutine`](crate::scheduled_routines::ScheduledRoutine) with
//! `trigger_type = "github"` carries a [`GitHubTrigger`] config and is fired
//! by the shannon-server `POST /hooks/github` endpoint when an incoming
//! GitHub webhook delivery matches the configured event, repository, and
//! (optionally) action.
//!
//! ## Routine config shape (serde contract, frozen)
//!
//! ```toml
//! # inside a scheduled task's definition / template
//! trigger_type = "github"
//! [github]
//! event = "issues"        # X-GitHub-Event header value
//! repo = "octocat/hello-world"  # repository.full_name; "*" matches any repo
//! action = "opened"       # optional; absent = any action
//! ```
//!
//! ## First-batch event mappings (frozen contract)
//!
//! | delivery                       | routine config                       |
//! |--------------------------------|--------------------------------------|
//! | `issues.opened`                | `{ event = "issues", action = "opened" }` |
//! | `issue_comment.created`        | `{ event = "issue_comment", action = "created" }` |
//! | `pull_request.opened`          | `{ event = "pull_request", action = "opened" }` |
//! | `check_run.completed` with `conclusion = "failure"` | `{ event = "check_run", action = "completed" }` |
//!
//! `check_run` deliveries with any other conclusion (success, cancelled, …)
//! are **not routable** in the first batch: [`GitHubEventInfo::parse`]
//! returns `None` for them, so no routine can match and the endpoint answers
//! `204`. Generalizing conclusion matching (e.g. `action = "completed:success"`)
//! is left to a later batch.

use serde::{Deserialize, Serialize};

use crate::scheduled_routines::{RoutineManager, ScheduledRoutine, TriggerType};

/// Routine-side GitHub trigger configuration (the `github` key on a
/// [`ScheduledRoutine`]). Present only when `trigger_type` is
/// [`TriggerType::Github`](crate::scheduled_routines::TriggerType::Github).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubTrigger {
    /// GitHub event name from the `X-GitHub-Event` header
    /// (`issues`, `issue_comment`, `pull_request`, `check_run`, …).
    pub event: String,
    /// Repository full name (`owner/name`) the delivery must come from.
    /// `"*"` matches any repository.
    pub repo: String,
    /// Optional action qualifier (`opened`, `created`, `completed`, …).
    /// Absent = match any action of the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// The subset of an incoming GitHub delivery used for routine matching.
///
/// Extracted by [`GitHubEventInfo::parse`] from the `X-GitHub-Event` header
/// plus the raw JSON payload (`repository.full_name`, `action`, and — for
/// `check_run` — `conclusion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubEventInfo {
    /// `X-GitHub-Event` header value (`issues`, `issue_comment`, …).
    pub event: String,
    /// `action` field of the payload (`opened`, `created`, `completed`, …).
    /// `None` for events that carry no action (e.g. `ping`).
    pub action: Option<String>,
    /// `repository.full_name` of the delivery.
    pub repo: String,
}

impl GitHubEventInfo {
    /// Parse matching info from the `X-GitHub-Event` header value and the raw
    /// payload.
    ///
    /// Returns `None` when the delivery cannot be routed in the first batch:
    /// - the header is empty, or
    /// - the payload carries no `repository.full_name`, or
    /// - it is a `check_run` delivery outside the frozen mapping
    ///   (`action != "completed"` or `conclusion != "failure"`).
    pub fn parse(event_header: &str, payload: &serde_json::Value) -> Option<Self> {
        if event_header.is_empty() {
            return None;
        }
        let repo = payload
            .get("repository")
            .and_then(|r| r.get("full_name"))
            .and_then(|v| v.as_str())?;
        let mut action = payload
            .get("action")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if event_header == "check_run" {
            // Frozen first-batch mapping: only completed failures are routable.
            // `conclusion` lives inside the `check_run` object on real
            // deliveries; also accept a top-level field defensively.
            let conclusion = payload
                .pointer("/check_run/conclusion")
                .or_else(|| payload.get("conclusion"))
                .and_then(|v| v.as_str());
            let is_failed_completion =
                action.as_deref() == Some("completed") && conclusion == Some("failure");
            if !is_failed_completion {
                return None;
            }
            action = Some("completed".to_string());
        }
        Some(Self {
            event: event_header.to_string(),
            action,
            repo: repo.to_string(),
        })
    }

    /// Wire label used in prompts and inbox titles, e.g. `issues.opened`.
    pub fn label(&self) -> String {
        match &self.action {
            Some(a) => format!("{}.{}", self.event, a),
            None => self.event.clone(),
        }
    }
}

/// Whether a single [`GitHubTrigger`] matches a parsed delivery.
///
/// - `event` must equal the delivery's event;
/// - `repo` must equal `repository.full_name` (`"*"` wildcard = any repo);
/// - `action`, when set, must equal the delivery's action (a trigger without
///   `action` matches any action).
pub fn github_trigger_matches(trigger: &GitHubTrigger, info: &GitHubEventInfo) -> bool {
    trigger.event == info.event
        && (trigger.repo == "*" || trigger.repo == info.repo)
        && trigger
            .action
            .as_deref()
            .is_none_or(|a| info.action.as_deref() == Some(a))
}

/// All enabled `github`-triggered routines matching a delivery, in store order.
///
/// Only routines with `trigger_type == TriggerType::Github`, `enabled = true`,
/// and a `github` config that matches are returned.
pub fn matching_github_routines(
    routines: &[ScheduledRoutine],
    info: &GitHubEventInfo,
) -> Vec<ScheduledRoutine> {
    routines
        .iter()
        .filter(|r| {
            r.enabled
                && r.trigger_type == TriggerType::Github
                && r.github
                    .as_ref()
                    .is_some_and(|t| github_trigger_matches(t, info))
        })
        .cloned()
        .collect()
}

impl RoutineManager {
    /// All enabled `github`-triggered routines in this manager matching a delivery.
    pub fn matching_github_routines(&self, info: &GitHubEventInfo) -> Vec<ScheduledRoutine> {
        let values: Vec<ScheduledRoutine> = self.routines.values().cloned().collect();
        matching_github_routines(&values, info)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::scheduled_routines::ScheduledRoutine;

    fn issue_opened_payload(repo: &str) -> serde_json::Value {
        serde_json::json!({
            "action": "opened",
            "issue": {
                "number": 1347,
                "title": "Bug: crash on save",
                "html_url": "https://github.com/octocat/hello-world/issues/1347",
                "user": { "login": "alice" }
            },
            "repository": { "id": 1, "full_name": repo },
            "sender": { "login": "alice" }
        })
    }

    // ── GitHubEventInfo::parse ──────────────────────────────────────────

    #[test]
    fn parse_issues_opened() {
        let info = GitHubEventInfo::parse("issues", &issue_opened_payload("octocat/hw")).unwrap();
        assert_eq!(info.event, "issues");
        assert_eq!(info.action.as_deref(), Some("opened"));
        assert_eq!(info.repo, "octocat/hw");
        assert_eq!(info.label(), "issues.opened");
    }

    #[test]
    fn parse_issue_comment_created() {
        let payload = serde_json::json!({
            "id": 1_000_000,
            "action": "created",
            "issue": { "number": 2, "title": "T" },
            "comment": { "id": 5, "body": "hi", "user": { "login": "bob" } },
            "repository": { "full_name": "octocat/hello-world" }
        });
        let info = GitHubEventInfo::parse("issue_comment", &payload).unwrap();
        assert_eq!(info.label(), "issue_comment.created");
        assert_eq!(info.repo, "octocat/hello-world");
    }

    #[test]
    fn parse_pull_request_opened() {
        let payload = serde_json::json!({
            "action": "opened",
            "number": 42,
            "pull_request": { "number": 42, "title": "Fix" },
            "repository": { "full_name": "octocat/hello-world" }
        });
        let info = GitHubEventInfo::parse("pull_request", &payload).unwrap();
        assert_eq!(info.label(), "pull_request.opened");
    }

    #[test]
    fn parse_check_run_completed_failure_is_routable() {
        let payload = serde_json::json!({
            "action": "completed",
            "check_run": {
                "name": "build",
                "conclusion": "failure",
                "check_suite": { "head_branch": "main" }
            },
            "repository": { "full_name": "octocat/hello-world" }
        });
        let info = GitHubEventInfo::parse("check_run", &payload).unwrap();
        assert_eq!(info.label(), "check_run.completed");
    }

    #[test]
    fn parse_check_run_success_is_not_routable() {
        let payload = serde_json::json!({
            "action": "completed",
            "check_run": { "name": "build", "conclusion": "success" },
            "repository": { "full_name": "octocat/hello-world" }
        });
        assert!(GitHubEventInfo::parse("check_run", &payload).is_none());
    }

    #[test]
    fn parse_check_run_other_action_is_not_routable() {
        let payload = serde_json::json!({
            "action": "created",
            "check_run": { "name": "build", "status": "in_progress", "conclusion": null },
            "repository": { "full_name": "octocat/hello-world" }
        });
        assert!(GitHubEventInfo::parse("check_run", &payload).is_none());
    }

    #[test]
    fn parse_without_repo_is_none() {
        let payload = serde_json::json!({ "action": "opened" });
        assert!(GitHubEventInfo::parse("issues", &payload).is_none());
    }

    #[test]
    fn parse_with_empty_header_is_none() {
        assert!(GitHubEventInfo::parse("", &issue_opened_payload("o/r")).is_none());
    }

    #[test]
    fn parse_ping_has_no_action() {
        let payload = serde_json::json!({
            "zen": "Keep it logically awesome.",
            "repository": { "full_name": "octocat/hello-world" }
        });
        let info = GitHubEventInfo::parse("ping", &payload).unwrap();
        assert_eq!(info.action, None);
        assert_eq!(info.label(), "ping");
    }

    // ── TriggerType::Github serde ───────────────────────────────────────

    #[test]
    fn trigger_type_github_serializes_lowercase() {
        let json = serde_json::to_string(&TriggerType::Github).unwrap();
        assert_eq!(json, r#""github""#);
        let back: TriggerType = serde_json::from_str(r#""github""#).unwrap();
        assert_eq!(back, TriggerType::Github);
    }

    // ── GitHubTrigger serde ─────────────────────────────────────────────

    #[test]
    fn github_trigger_serde_roundtrip_with_action() {
        let trigger = GitHubTrigger {
            event: "issues".into(),
            repo: "octocat/hello-world".into(),
            action: Some("opened".into()),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        assert!(json.contains(r#""event":"issues""#));
        let back: GitHubTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(back, trigger);
    }

    #[test]
    fn github_trigger_action_is_optional() {
        let trigger: GitHubTrigger =
            serde_json::from_str(r#"{ "event": "check_run", "repo": "o/r" }"#).unwrap();
        assert_eq!(trigger.action, None);
    }

    // ── Matching ────────────────────────────────────────────────────────

    fn routine_with_github(id: &str, trigger: GitHubTrigger, enabled: bool) -> ScheduledRoutine {
        let mut r = ScheduledRoutine::new(id.to_string(), "triage prompt".to_string(), 0);
        r.id = id.to_string();
        r.trigger_type = TriggerType::Github;
        r.enabled = enabled;
        r.github = Some(trigger);
        r
    }

    fn info(event: &str, action: &str, repo: &str) -> GitHubEventInfo {
        GitHubEventInfo {
            event: event.to_string(),
            action: Some(action.to_string()),
            repo: repo.to_string(),
        }
    }

    #[test]
    fn matching_selects_event_repo_action() {
        let routines = vec![routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issues".into(),
                repo: "octocat/hello-world".into(),
                action: Some("opened".into()),
            },
            true,
        )];
        let matched =
            matching_github_routines(&routines, &info("issues", "opened", "octocat/hello-world"));
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].id, "r1");

        // Wrong action / wrong repo / wrong event → no match.
        assert!(
            matching_github_routines(&routines, &info("issues", "labeled", "octocat/hello-world"))
                .is_empty()
        );
        assert!(
            matching_github_routines(&routines, &info("issues", "opened", "other/repo")).is_empty()
        );
        assert!(
            matching_github_routines(
                &routines,
                &info("pull_request", "opened", "octocat/hello-world")
            )
            .is_empty()
        );
    }

    #[test]
    fn matching_without_action_matches_any_action() {
        let routines = vec![routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issue_comment".into(),
                repo: "o/r".into(),
                action: None,
            },
            true,
        )];
        assert_eq!(
            matching_github_routines(&routines, &info("issue_comment", "created", "o/r")).len(),
            1
        );
        assert_eq!(
            matching_github_routines(&routines, &info("issue_comment", "edited", "o/r")).len(),
            1
        );
    }

    #[test]
    fn matching_repo_wildcard() {
        let routines = vec![routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issues".into(),
                repo: "*".into(),
                action: Some("opened".into()),
            },
            true,
        )];
        assert_eq!(
            matching_github_routines(&routines, &info("issues", "opened", "a/b")).len(),
            1
        );
        assert_eq!(
            matching_github_routines(&routines, &info("issues", "opened", "c/d")).len(),
            1
        );
    }

    #[test]
    fn matching_skips_disabled_and_non_github_routines() {
        let disabled = routine_with_github(
            "r-disabled",
            GitHubTrigger {
                event: "issues".into(),
                repo: "*".into(),
                action: Some("opened".into()),
            },
            false,
        );
        let mut interval = ScheduledRoutine::new("ri".into(), "p".into(), 60);
        interval.github = Some(GitHubTrigger {
            event: "issues".into(),
            repo: "*".into(),
            action: Some("opened".into()),
        });
        let routines = vec![disabled, interval];
        assert!(matching_github_routines(&routines, &info("issues", "opened", "a/b")).is_empty());
    }

    #[test]
    fn manager_matching_github_routines() {
        let mut mgr = RoutineManager::new();
        mgr.add(routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issues".into(),
                repo: "*".into(),
                action: Some("opened".into()),
            },
            true,
        ));
        mgr.add(ScheduledRoutine::new("r2".into(), "p".into(), 60));
        assert_eq!(
            mgr.matching_github_routines(&info("issues", "opened", "a/b"))
                .len(),
            1
        );
    }

    // ── Github routines never fire from the time-based drain ────────────

    #[test]
    fn github_routine_should_not_fire_by_time() {
        let r = routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issues".into(),
                repo: "*".into(),
                action: None,
            },
            true,
        );
        assert!(
            !r.should_fire(),
            "event-driven routines must not fire from drain_due"
        );
    }

    #[test]
    fn github_routine_serde_roundtrip() {
        let mut r = routine_with_github(
            "r1",
            GitHubTrigger {
                event: "issues".into(),
                repo: "o/r".into(),
                action: Some("opened".into()),
            },
            true,
        );
        r.trigger_type = TriggerType::Github;
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""trigger_type":"github""#), "{json}");
        assert!(json.contains(r#""event":"issues""#), "{json}");
        let back: ScheduledRoutine = serde_json::from_str(&json).unwrap();
        assert_eq!(back.trigger_type, TriggerType::Github);
        assert_eq!(back.github.unwrap().repo, "o/r");
    }
}
