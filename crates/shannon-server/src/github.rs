//! GitHub webhook receiver: `POST /hooks/github` (P2-7).
//!
//! Receives GitHub webhook deliveries, verifies the `X-Hub-Signature-256`
//! HMAC against `[hooks.github] secret` from `~/.shannon/config.toml`,
//! matches the delivery against `github`-triggered scheduled routines, and
//! executes each match in-process via the same session-creation + message
//! path as `POST /v1/sessions` (a targeted lift of the serve-mode 501
//! limitation from P0-3 — see `trigger_routine` for the sibling endpoint
//! that keeps the 501 semantics).
//!
//! Frozen response contract:
//! - matched   → `202 {"runIds": [...]}`
//! - unmatched → `204` (no body)
//! - bad/missing signature → `401`
//! - `[hooks.github] secret` not configured → `503`
//!
//! Idempotency: `X-GitHub-Delivery` ids are deduped in a bounded in-memory
//! replay cache; a redelivered id returns the original `202` runIds without
//! re-executing. The cache is intentionally not persisted — a serve restart
//! forgets seen ids (GitHub redeliveries may re-execute then); persistence
//! (e.g. a `meta` row in the shared inbox DB) is a deliberate non-goal for
//! the MVP and is documented in `docs/integrations/github-triggers.md`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use shannon_core::github_triggers::{GitHubEventInfo, matching_github_routines};
use shannon_core::inbox_store::{InboxItemNew, InboxStore};
use shannon_core::query_engine::{QueryContext, QueryEvent};
use shannon_core::scheduled_routines::ScheduledRoutine;

use crate::AppState;

/// Maximum entries in the delivery replay cache (oldest evicted first).
const DELIVERY_CACHE_CAP: usize = 4096;

pub(crate) const GITHUB_HOOK_PATH: &str = "/hooks/github";

const SIGNATURE_HEADER: &str = "X-Hub-Signature-256";
const EVENT_HEADER: &str = "X-GitHub-Event";
const DELIVERY_HEADER: &str = "X-GitHub-Delivery";

const DOC_NOTE: &str = "see docs/integrations/github-triggers.md";

// ── Delivery replay cache ───────────────────────────────────────────────

/// Bounded FIFO cache of recently seen `X-GitHub-Delivery` ids mapping to
/// the runIds accepted for them. Not persistent (see module docs).
#[derive(Default)]
pub struct DeliveryCache {
    entries: HashMap<String, Vec<String>>,
    order: VecDeque<String>,
    cap: usize,
}

impl DeliveryCache {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    /// RunIds previously accepted for this delivery id, if any.
    pub fn get(&self, delivery: &str) -> Option<&Vec<String>> {
        self.entries.get(delivery)
    }

    /// Record a delivery and its runIds, evicting the oldest entry beyond capacity.
    pub fn insert(&mut self, delivery: String, run_ids: Vec<String>) {
        if self.entries.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(delivery.clone());
        self.entries.insert(delivery, run_ids);
    }
}

// ── Response schemas ────────────────────────────────────────────────────

/// `202` body: one run id per matched routine.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubHookAccepted {
    pub run_ids: Vec<String>,
}

/// Error body for 401/503.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GitHubHookError {
    pub error: String,
    /// Present on 503: where the setup docs live.
    pub doc: Option<String>,
}

fn error_response(status: StatusCode, error: String, doc: Option<String>) -> Response {
    (status, Json(GitHubHookError { error, doc })).into_response()
}

// ── Handler ─────────────────────────────────────────────────────────────

/// GitHub webhook receiver (P2-7).
///
/// Auth is the GitHub `X-Hub-Signature-256` HMAC over the **raw** body,
/// verified against `[hooks.github] secret` from `~/.shannon/config.toml`
/// (shared verification logic lives in [`shannon_core::webhook`]). The
/// endpoint is exempt from the server's bearer middleware — GitHub cannot
/// send our bearer token; when the secret is not configured the endpoint
/// answers **503** (disabled, safe default).
///
/// First-batch event mappings (frozen): `issues.opened`,
/// `issue_comment.created`, `pull_request.opened`,
/// `check_run.completed(conclusion=failure)`. Routines opt in via
/// `trigger_type = "github"` plus the `github = { event, repo, action? }`
/// config in their task definition.
#[utoipa::path(
    post,
    path = "/hooks/github",
    responses(
        (status = 202, description = "Delivery matched at least one routine; runs started", body = GitHubHookAccepted),
        (status = 204, description = "Signature valid but no routine matches the event/repo/action"),
        (status = 400, description = "Body is not valid JSON (defensive; signed senders should not hit this)", body = GitHubHookError),
        (status = 401, description = "Missing or invalid X-Hub-Signature-256", body = GitHubHookError),
        (status = 503, description = "Disabled: no [hooks.github] secret configured", body = GitHubHookError)
    )
)]
pub async fn github_hook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(secret) = state.github_secret.as_deref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "github webhook endpoint disabled: no [hooks.github] secret configured in ~/.shannon/config.toml"
                .to_string(),
            Some(DOC_NOTE.to_string()),
        );
    };

    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !shannon_core::webhook::verify_signature(secret, &body, signature) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            format!("missing or invalid {SIGNATURE_HEADER} header"),
            None,
        );
    }

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid JSON payload: {e}"),
                None,
            );
        }
    };

    let event_header = headers
        .get(EVENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(info) = GitHubEventInfo::parse(event_header, &payload) else {
        // Not routable in the first batch (other conclusions/actions, no
        // repository, ping, …) — an expected, successful no-op for GitHub.
        return StatusCode::NO_CONTENT.into_response();
    };

    let delivery = headers
        .get(DELIVERY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Idempotency: a redelivered id returns the original acceptance.
    if !delivery.is_empty() {
        let cached = state
            .github_deliveries
            .lock()
            .ok()
            .and_then(|cache| cache.get(delivery).cloned());
        if let Some(run_ids) = cached {
            tracing::info!(delivery, "replaying cached GitHub delivery acceptance");
            return (StatusCode::ACCEPTED, Json(GitHubHookAccepted { run_ids })).into_response();
        }
    }

    let matched = matching_github_routines(&state.routines, &info);
    if matched.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }

    let mut run_ids = Vec::with_capacity(matched.len());
    for routine in matched {
        let run_id = spawn_github_routine_run(&state, routine, &info, &payload).await;
        run_ids.push(run_id);
    }

    if !delivery.is_empty() {
        if let Ok(mut cache) = state.github_deliveries.lock() {
            cache.insert(delivery.to_string(), run_ids.clone());
        }
    }

    (StatusCode::ACCEPTED, Json(GitHubHookAccepted { run_ids })).into_response()
}

// ── Serve-side execution (targeted lift of the P0-3 501 ruling) ─────────

/// Record a run and spawn its execution in the serve process.
///
/// Execution mirrors `POST /v1/sessions` + `POST /v1/sessions/{id}/messages`:
/// a fresh `QueryEngine` (same constructor, same bare `ToolRegistry`) is
/// created, the routine prompt (plus a GitHub event context block) is run
/// through `process_query`, and the outcome is written to the shared
/// [`InboxStore`] (`~/.shannon/inbox.db`) with a `routine_runs` record — so
/// desktop and serve see the same inbox/run history.
///
/// Returns the run id immediately (the caller answers `202`); execution
/// continues in the background and always finalizes the run (`succeeded` /
/// `failed`), including on engine panic.
async fn spawn_github_routine_run(
    state: &AppState,
    routine: ScheduledRoutine,
    info: &GitHubEventInfo,
    payload: &serde_json::Value,
) -> String {
    let inbox = state.inbox.clone();
    let run_id = match inbox.record_run_start(&routine.id, &routine.name) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(
                routine = %routine.id,
                error = %e,
                "could not record routine run start; continuing with a synthetic run id"
            );
            uuid::Uuid::new_v4().to_string()
        }
    };

    let title = describe_event(info, payload);
    let prompt = build_prompt(&routine.prompt, info, payload);
    let client_config = state.client_config.clone();
    let sessions = state.sessions.clone();

    let started = Instant::now();
    // Session creation identical to POST /v1/sessions.
    let engine = crate::routes::build_engine(client_config);
    let session_id = sessions.create(engine).await.id;

    let run_id_for_task = run_id.clone();
    let ctx = RunContext {
        title,
        session_id,
        started,
    };

    tokio::spawn(async move {
        // Engine stage in a nested task with a panic guard (the P0-3 fix-round
        // pattern): a panicking query must still finalize the run as failed.
        let engine_task = tokio::spawn(async move {
            let mut text = String::new();
            let mut failure: Option<String> = None;
            if let Some(session) = sessions.get(session_id).await {
                let metadata = shannon_core::query_engine::QueryMetadata {
                    timestamp: chrono::Utc::now(),
                    tools_allowed: true,
                    max_tokens: None,
                    model: session.engine.lock().await.client().model().to_string(),
                    temperature: None,
                    top_p: None,
                };
                let context = QueryContext {
                    query_id: uuid::Uuid::new_v4(),
                    session_id,
                    user_message: prompt,
                    metadata,
                };
                use futures::StreamExt;
                let mut stream = session
                    .engine
                    .lock()
                    .await
                    .process_query(context, None)
                    .await;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(QueryEvent::Text { content, .. }) => text.push_str(&content),
                        Ok(QueryEvent::Failed { error, .. }) => {
                            failure = Some(error);
                            break;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            failure = Some(e.to_string());
                            break;
                        }
                    }
                }
            } else {
                failure = Some("session vanished before execution".to_string());
            }
            RunOutcome { text, failure }
        });

        let outcome = match engine_task.await {
            Ok(outcome) => outcome,
            Err(join_error) => RunOutcome {
                text: String::new(),
                failure: Some(format!("routine task panicked: {join_error}")),
            },
        };

        finalize_run(&inbox, &run_id_for_task, &routine, ctx, outcome);
    });

    run_id
}

/// Where and when a run started (used by [`finalize_run`]).
struct RunContext {
    title: String,
    session_id: uuid::Uuid,
    started: Instant,
}

/// What the engine stage produced.
struct RunOutcome {
    text: String,
    failure: Option<String>,
}

/// Write the inbox item and close the `routine_runs` record.
///
/// Single choke point so every path (success, engine failure, panic, store
/// error) terminates the run instead of leaving it `running` forever.
fn finalize_run(
    inbox: &InboxStore,
    run_id: &str,
    routine: &ScheduledRoutine,
    ctx: RunContext,
    outcome: RunOutcome,
) {
    let RunContext {
        title,
        session_id,
        started,
    } = ctx;
    let RunOutcome { text, failure } = outcome;
    let took = started.elapsed().as_secs();
    let outcome_line = summarize_output(&text, failure.as_deref());
    let summary = format!("took {took}s · {outcome_line}");

    let appended = inbox.append_item(InboxItemNew {
        source: "routine".to_string(),
        source_id: Some(routine.id.clone()),
        session_id: Some(session_id.to_string()),
        title: title.to_string(),
        summary,
        error: failure.clone(),
    });
    let inbox_item_id = match appended {
        Ok(item) => Some(item.id),
        Err(e) => {
            tracing::warn!(run_id, error = %e, "could not append inbox item");
            None
        }
    };

    let status = if failure.is_none() {
        "succeeded"
    } else {
        "failed"
    };
    if let Err(e) = inbox.record_run_finish(run_id, status, failure.as_deref(), inbox_item_id) {
        tracing::warn!(run_id, error = %e, "could not record run finish");
    }
}

/// Single-line output summary (≤500 chars, UTF-8 safe).
///
/// Success: first non-empty output line. Failure: the last non-empty output
/// line (error details usually trail) or the failure message itself.
fn summarize_output(text: &str, failure: Option<&str>) -> String {
    let line = match failure {
        None => text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("(no output)"),
        Some(_) => text
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or(""),
    };
    let mut line = if line.is_empty() {
        failure.unwrap_or("(no output)").to_string()
    } else {
        line.to_string()
    };
    if line.chars().count() > 500 {
        line = line.chars().take(499).collect::<String>() + "…";
    }
    line
}

// ── Prompt composition ──────────────────────────────────────────────────

/// Human-readable one-line description of the delivery (inbox title).
fn describe_event(info: &GitHubEventInfo, payload: &serde_json::Value) -> String {
    let repo = &info.repo;
    let num = |pointer: &str| {
        payload
            .pointer(pointer)
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let s = |pointer: &str| {
        payload
            .pointer(pointer)
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };
    match (info.event.as_str(), info.action.as_deref()) {
        ("issues", action) => format!(
            "Issue #{} \u{201c}{}\u{201d} {} by @{} in {repo}",
            num("/issue/number"),
            s("/issue/title"),
            action.unwrap_or("updated"),
            s("/sender/login"),
        ),
        ("issue_comment", _) => format!(
            "@{} commented on #{} in {repo}: {}",
            s("/comment/user/login"),
            num("/issue/number"),
            first_line(s("/comment/body")),
        ),
        ("pull_request", action) => format!(
            "PR #{} \u{201c}{}\u{201d} {} by @{} in {repo}",
            num("/pull_request/number"),
            s("/pull_request/title"),
            action.unwrap_or("updated"),
            s("/sender/login"),
        ),
        ("check_run", _) => format!(
            "Check \u{201c}{}\u{201d} failed ({}) in {repo}: {}",
            s("/check_run/name"),
            s("/check_run/check_suite/head_branch"),
            first_line(s("/check_run/output/title")),
        ),
        _ => format!("GitHub event `{}` in {repo}", info.label()),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// Compose the message executed for a matched routine: the routine's prompt
/// plus a GitHub event context block.
fn build_prompt(
    routine_prompt: &str,
    info: &GitHubEventInfo,
    payload: &serde_json::Value,
) -> String {
    let description = describe_event(info, payload);
    let url = ["issue", "pull_request", "comment", "check_run"]
        .iter()
        .find_map(|seg| payload.pointer(&format!("/{seg}/html_url")))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut prompt = format!(
        "{routine_prompt}\n\n---\nTrigger: GitHub `{label}` in `{repo}`.\n{description}",
        label = info.label(),
        repo = info.repo,
        description = description,
    );
    if !url.is_empty() {
        prompt.push_str("\nURL: ");
        prompt.push_str(url);
    }
    prompt
}

// ── Router wiring helpers (production construction lives in lib.rs) ─────

/// Load the routine snapshot served to the GitHub hook from the shared
/// scheduled-task store (`~/.shannon/scheduled-tasks/`). Missing store →
/// empty snapshot (all deliveries then answer 204).
pub fn load_routines() -> Vec<ScheduledRoutine> {
    match shannon_core::scheduled_task_store::ScheduledTaskStore::new().list() {
        Ok(routines) => routines,
        Err(e) => {
            tracing::warn!(error = %e, "could not list scheduled tasks; github triggers disabled");
            Vec::new()
        }
    }
}

/// Open the shared inbox store, degrading to an in-memory DB when the shared
/// file cannot be opened (serve still runs, but results are not visible to
/// the desktop).
pub fn open_inbox() -> InboxStore {
    match InboxStore::open_default() {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!(error = %e, "could not open ~/.shannon/inbox.db; using an in-memory inbox");
            InboxStore::open_in_memory().expect("in-memory inbox cannot fail to open")
        }
    }
}

/// Convenience constructor matching [`AppState`]'s needs.
pub fn delivery_cache() -> Arc<Mutex<DeliveryCache>> {
    Arc::new(Mutex::new(DeliveryCache::new(DELIVERY_CACHE_CAP)))
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::router_full;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use shannon_core::github_triggers::GitHubTrigger;
    use shannon_core::scheduled_routines::{ScheduledRoutine, TriggerType};
    use tower::ServiceExt;

    const SECRET: &str = "gh-s3cret";

    fn test_config() -> shannon_engine::api::LlmClientConfig {
        shannon_core::LlmClientConfig {
            provider: shannon_engine::api::types::LlmProvider::Ollama,
            model: "test-model".into(),
            base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        }
    }

    // ── Realistic GitHub payload fixtures (minimal real field shape) ────

    fn issues_opened() -> serde_json::Value {
        serde_json::json!({
            "action": "opened",
            "issue": {
                "id": 2_543_928_114u64,
                "number": 1347,
                "title": "Bug: crash on save with large file",
                "user": { "login": "alice" },
                "state": "open",
                "html_url": "https://github.com/octocat/hello-world/issues/1347",
                "body": "Repro: open a 50MB file and hit save."
            },
            "repository": {
                "id": 1296269,
                "full_name": "octocat/hello-world",
                "html_url": "https://github.com/octocat/hello-world"
            },
            "sender": { "login": "alice" }
        })
    }

    fn issue_comment_created() -> serde_json::Value {
        serde_json::json!({
            "id": 1_234_567_890u64,
            "action": "created",
            "issue": {
                "number": 2,
                "title": "Spelling error in the README file",
                "html_url": "https://github.com/octocat/hello-world/issues/2"
            },
            "comment": {
                "id": 9_876_543_210u64,
                "body": "Please triage: this blocks the 1.0 release.",
                "user": { "login": "bob" },
                "html_url": "https://github.com/octocat/hello-world/issues/2#issuecomment-9876543210"
            },
            "repository": { "id": 1296269, "full_name": "octocat/hello-world" },
            "sender": { "login": "bob" }
        })
    }

    fn pull_request_opened() -> serde_json::Value {
        serde_json::json!({
            "action": "opened",
            "number": 42,
            "pull_request": {
                "id": 8_765_432_109u64,
                "number": 42,
                "title": "Fix save crash for large files",
                "user": { "login": "carol" },
                "html_url": "https://github.com/octocat/hello-world/pull/42",
                "body": "Stream writes so we never buffer the whole file."
            },
            "repository": { "id": 1296269, "full_name": "octocat/hello-world" },
            "sender": { "login": "carol" }
        })
    }

    fn check_run_completed(conclusion: &str) -> serde_json::Value {
        serde_json::json!({
            "action": "completed",
            "check_run": {
                "id": 1_234_567_890u64,
                "name": "build",
                "conclusion": conclusion,
                "html_url": "https://github.com/octocat/hello-world/actions/runs/123/job/456",
                "check_suite": { "id": 987, "head_branch": "main" },
                "head_sha": "d6fde92930d4715a2b49857d24b940956b26d2d3",
                "output": { "title": "Process completed with exit code 1." }
            },
            "repository": { "id": 1296269, "full_name": "octocat/hello-world" },
            "sender": { "login": "github-actions[bot]" }
        })
    }

    // ── Harness ─────────────────────────────────────────────────────────

    fn github_routine(id: &str, event: &str, repo: &str, action: Option<&str>) -> ScheduledRoutine {
        let mut r = ScheduledRoutine::new(id.to_string(), "Triage this GitHub event.".into(), 0);
        r.id = id.to_string();
        r.trigger_type = TriggerType::Github;
        r.github = Some(GitHubTrigger {
            event: event.to_string(),
            repo: repo.to_string(),
            action: action.map(str::to_string),
        });
        r
    }

    struct TestApp {
        app: axum::Router,
        inbox: Arc<InboxStore>,
    }

    fn test_app(github_secret: Option<&str>, routines: Vec<ScheduledRoutine>) -> TestApp {
        let inbox = Arc::new(InboxStore::open_in_memory().unwrap());
        let app = router_full(
            test_config(),
            None,
            None,
            github_secret.map(str::to_string),
            routines,
            inbox.clone(),
        );
        TestApp { app, inbox }
    }

    /// Send a signed GitHub delivery. `signature` of `Some(None)` sends a
    /// garbage signature, `None` omits the header.
    async fn deliver(
        TestApp { app, .. }: &TestApp,
        event: &str,
        delivery: &str,
        payload: &serde_json::Value,
        signature: Option<Option<&str>>,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::to_vec(payload).unwrap();
        let mut req = Request::builder()
            .method("POST")
            .uri(GITHUB_HOOK_PATH)
            .header("content-type", "application/json")
            .header(EVENT_HEADER, event);
        if !delivery.is_empty() {
            req = req.header(DELIVERY_HEADER, delivery);
        }
        match signature {
            Some(Some(sig)) => req = req.header(SIGNATURE_HEADER, sig),
            Some(None) => req = req.header(SIGNATURE_HEADER, "sha256=deadbeef"),
            None => {}
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, json)
    }

    fn sign(payload: &serde_json::Value) -> String {
        shannon_core::webhook::sign_signature(SECRET, &serde_json::to_vec(payload).unwrap())
    }

    /// Wait (bounded) for the spawned run of `run_id` to leave `running`.
    /// Generous bound: the engine's retry policy (3 retries with exponential
    /// backoff) applies before a failed run finalizes.
    async fn wait_for_run(inbox: &InboxStore, run_id: &str) {
        for _ in 0..300 {
            let runs = inbox.list_runs(50).unwrap();
            if runs.iter().any(|r| r.id == run_id && r.status != "running") {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("run {run_id} never finalized");
    }

    // ── Auth: 503 / 401 ─────────────────────────────────────────────────

    #[tokio::test]
    async fn github_hook_without_secret_is_503() {
        let harness = test_app(None, vec![]);
        let (status, body) = deliver(&harness, "issues", "d-1", &issues_opened(), None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(body["error"].as_str().unwrap().contains("secret"));
        assert_eq!(body["doc"].as_str(), Some(DOC_NOTE));
    }

    #[tokio::test]
    async fn github_hook_missing_signature_is_401() {
        let harness = test_app(Some(SECRET), vec![]);
        let (status, body) = deliver(&harness, "issues", "d-1", &issues_opened(), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }

    #[tokio::test]
    async fn github_hook_tampered_signature_is_401() {
        let harness = test_app(Some(SECRET), vec![]);
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-1",
            &issues_opened(),
            Some(Some(
                "sha256=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            )),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // Right signature, wrong secret.
        let other = shannon_core::webhook::sign_signature(
            "not-the-secret",
            &serde_json::to_vec(&issues_opened()).unwrap(),
        );
        let harness = test_app(Some(SECRET), vec![]);
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-1",
            &issues_opened(),
            Some(Some(&other)),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }

    #[tokio::test]
    async fn github_hook_is_exempt_from_bearer_middleware() {
        // Token configured: other routes require the bearer, but the GitHub
        // hook must stay reachable with just its HMAC signature.
        let inbox = Arc::new(InboxStore::open_in_memory().unwrap());
        let app = router_full(
            test_config(),
            Some("tok".into()),
            None,
            Some(SECRET.to_string()),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
            inbox.clone(),
        );
        let payload = issues_opened();
        let req = Request::builder()
            .method("POST")
            .uri(GITHUB_HOOK_PATH)
            .header("content-type", "application/json")
            .header(EVENT_HEADER, "issues")
            .header(DELIVERY_HEADER, "d-bearer")
            .header(SIGNATURE_HEADER, sign(&payload))
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    // ── The four frozen event mappings ─────────────────────────────────

    #[tokio::test]
    async fn issues_opened_triggers_routine_and_writes_inbox() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        let delivery = "d-issues-1";
        let (status, body) = deliver(
            &harness,
            "issues",
            delivery,
            &issues_opened(),
            Some(Some(&sign(&issues_opened()))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let run_ids = body["runIds"].as_array().unwrap();
        assert_eq!(run_ids.len(), 1);
        let run_id = run_ids[0].as_str().unwrap().to_string();

        wait_for_run(&harness.inbox, &run_id).await;
        let items = harness.inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1, "expected exactly one inbox item");
        assert_eq!(items[0].source, "routine");
        assert_eq!(items[0].source_id.as_deref(), Some("r1"));
        assert_eq!(items[0].status, "pending");
        assert!(items[0].title.contains("#1347"), "{}", items[0].title);
        assert!(
            items[0].session_id.is_some(),
            "session id recorded for continue"
        );
    }

    #[tokio::test]
    async fn issue_comment_created_triggers_routine() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issue_comment",
                "octocat/hello-world",
                Some("created"),
            )],
        );
        let payload = issue_comment_created();
        let (status, body) = deliver(
            &harness,
            "issue_comment",
            "d-c1",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["runIds"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pull_request_opened_triggers_routine() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "pull_request",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        let payload = pull_request_opened();
        let (status, body) = deliver(
            &harness,
            "pull_request",
            "d-pr1",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["runIds"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn check_run_completed_failure_triggers_routine() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "check_run",
                "octocat/hello-world",
                Some("completed"),
            )],
        );
        let payload = check_run_completed("failure");
        let (status, body) = deliver(
            &harness,
            "check_run",
            "d-cc1",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["runIds"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn check_run_completed_success_is_ignored() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "check_run",
                "octocat/hello-world",
                Some("completed"),
            )],
        );
        let payload = check_run_completed("success");
        let (status, body) = deliver(
            &harness,
            "check_run",
            "d-cc2",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    // ── Matching / unmatched ────────────────────────────────────────────

    #[tokio::test]
    async fn unmatched_event_is_204() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        // Wrong action.
        let mut labeled = issues_opened();
        labeled["action"] = "labeled".into();
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-2",
            &labeled,
            Some(Some(&sign(&labeled))),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

        // Wrong repo.
        let harness = test_app(
            Some(SECRET),
            vec![github_routine("r1", "issues", "other/repo", Some("opened"))],
        );
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-3",
            &issues_opened(),
            Some(Some(&sign(&issues_opened()))),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

        // Unknown event.
        let (status, body) = deliver(
            &harness,
            "fork",
            "d-4",
            &issues_opened(),
            Some(Some(&sign(&issues_opened()))),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    #[tokio::test]
    async fn multiple_matching_routines_all_trigger() {
        let harness = test_app(
            Some(SECRET),
            vec![
                github_routine("r1", "issues", "octocat/hello-world", Some("opened")),
                github_routine("r2", "issues", "*", Some("opened")),
            ],
        );
        let payload = issues_opened();
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-5",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["runIds"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn disabled_routine_does_not_trigger() {
        let mut r = github_routine("r1", "issues", "octocat/hello-world", Some("opened"));
        r.enabled = false;
        let harness = test_app(Some(SECRET), vec![r]);
        let payload = issues_opened();
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-6",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    // ── Idempotency ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn delivery_replay_returns_same_run_ids_without_reexecution() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        let payload = issues_opened();
        let sig = sign(&payload);
        let (status1, body1) =
            deliver(&harness, "issues", "d-replay", &payload, Some(Some(&sig))).await;
        let (status2, body2) =
            deliver(&harness, "issues", "d-replay", &payload, Some(Some(&sig))).await;
        assert_eq!(status1, StatusCode::ACCEPTED);
        assert_eq!(status2, StatusCode::ACCEPTED);
        assert_eq!(body1, body2, "replay must return the original runIds");

        wait_for_run(&harness.inbox, body1["runIds"][0].as_str().unwrap()).await;
        // Only one run recorded — the replay did not execute again.
        assert_eq!(harness.inbox.list_runs(50).unwrap().len(), 1);
        assert_eq!(harness.inbox.list(None, None, 10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_delivery_header_skips_dedupe() {
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        let payload = issues_opened();
        let sig = sign(&payload);
        let (s1, b1) = deliver(&harness, "issues", "", &payload, Some(Some(&sig))).await;
        let (s2, b2) = deliver(&harness, "issues", "", &payload, Some(Some(&sig))).await;
        assert_eq!(s1, StatusCode::ACCEPTED);
        assert_eq!(s2, StatusCode::ACCEPTED);
        assert_ne!(b1, b2, "distinct deliveries must get distinct run ids");
    }

    // ── Execution outcomes ──────────────────────────────────────────────

    #[tokio::test]
    async fn engine_failure_still_finalizes_run_and_inbox_item() {
        // test_config points at http://127.0.0.1:1 — the engine fails fast;
        // the run must end `failed` with an inbox item carrying the error.
        let harness = test_app(
            Some(SECRET),
            vec![github_routine(
                "r1",
                "issues",
                "octocat/hello-world",
                Some("opened"),
            )],
        );
        let payload = issues_opened();
        let (status, body) = deliver(
            &harness,
            "issues",
            "d-f1",
            &payload,
            Some(Some(&sign(&payload))),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let run_id = body["runIds"][0].as_str().unwrap().to_string();

        wait_for_run(&harness.inbox, &run_id).await;
        let runs = harness.inbox.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "failed", "run must reach a terminal state");
        assert!(runs[0].error.is_some());

        let items = harness.inbox.list(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].error.is_some(), "inbox item carries the error");
        assert_eq!(runs[0].task_id, "r1");
    }

    // ── Prompt composition ──────────────────────────────────────────────

    #[tokio::test]
    async fn github_hook_route_is_documented_in_openapi() {
        let harness = test_app(Some(SECRET), vec![]);
        let res = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let openapi: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            openapi["paths"]["/hooks/github"]["post"].is_object(),
            "/hooks/github missing from OpenAPI"
        );
        for status in ["202", "204", "401", "503"] {
            assert!(
                openapi["paths"]["/hooks/github"]["post"]["responses"][status].is_object(),
                "OpenAPI must document the {status} response of /hooks/github"
            );
        }
    }

    #[test]
    fn build_prompt_includes_routine_prompt_and_event_context() {
        let info = GitHubEventInfo::parse("issues", &issues_opened()).unwrap();
        let prompt = build_prompt("Summarize and triage.", &info, &issues_opened());
        assert!(prompt.starts_with("Summarize and triage."));
        assert!(prompt.contains("`issues.opened`"));
        assert!(prompt.contains("octocat/hello-world"));
        assert!(prompt.contains("#1347"));
        assert!(prompt.contains("https://github.com/octocat/hello-world/issues/1347"));
    }

    #[test]
    fn describe_event_covers_four_mappings() {
        let cases = [
            ("issues", issues_opened(), "#1347"),
            ("issue_comment", issue_comment_created(), "@bob"),
            ("pull_request", pull_request_opened(), "PR #42"),
            ("check_run", check_run_completed("failure"), "build"),
        ];
        for (event, payload, needle) in cases {
            let info = GitHubEventInfo::parse(event, &payload).unwrap();
            let title = describe_event(&info, &payload);
            assert!(title.contains(needle), "{event}: {title}");
        }
    }

    #[test]
    fn summarize_output_truncates_and_picks_lines() {
        let long = "x".repeat(600);
        let s = summarize_output(&long, None);
        assert_eq!(s.chars().count(), 500);
        assert!(s.ends_with('…'));

        let s = summarize_output("first\n\nsecond\n", None);
        assert_eq!(s, "first");

        let s = summarize_output("warned once\nboom: exit 1\n", Some("engine failed"));
        assert_eq!(s, "boom: exit 1");

        let s = summarize_output("", Some("connection refused"));
        assert_eq!(s, "connection refused");
    }

    #[test]
    fn delivery_cache_evicts_oldest_beyond_capacity() {
        let mut cache = DeliveryCache::new(2);
        cache.insert("a".into(), vec!["1".into()]);
        cache.insert("b".into(), vec!["2".into()]);
        cache.insert("c".into(), vec!["3".into()]);
        assert!(cache.get("a").is_none(), "oldest evicted");
        assert_eq!(cache.get("b").unwrap(), &vec!["2".to_string()]);
        assert_eq!(cache.get("c").unwrap(), &vec!["3".to_string()]);
    }
}
