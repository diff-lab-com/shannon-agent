//! Legacy ① — LLM session title generation: the first real consumer of the
//! `AuxRole::TitleGeneration` utility slot. S3-3 (PR #312) landed the slot's
//! schema, write path and UI with **zero** consumption points (the honest
//! verdict in `utility_tier`'s module docs); this module closes that gap.
//!
//! ## Behavior contract (default = byte-for-byte the dev behavior)
//!
//! - **Slot unconfigured (the default)**: nothing here ever runs. The
//!   resolver returns `None` before any attempt is marked, no LLM call is
//!   made, and the session title stays the deterministic Tier-1 truncation
//!   (`derive_title_from_message`) — zero behavior change for users who
//!   never touch Settings → Models → Utility slots.
//! - **Slot configured**: after the session's **first** query settles (any
//!   terminal state: ok / failed / cancelled / caught panic — the hook sits
//!   on the send path's always-runs flag reset), a fire-and-forget task
//!   asks the SLOT target (provider/base_url/credential/model all from
//!   `lookup_auxiliary_target`, never the interactive chain) for a short
//!   title: one small non-streaming request (`max_tokens` 32, 5s total
//!   deadline, thinking disabled where the wire supports the toggle).
//!   Success → the title is applied through the same persistence path the
//!   Tier-1 auto-title and `rename_session` use (sessions vec + sidecar
//!   merge + `sessions:updated`). Failure/timeout → `warn` + silently keep
//!   the deterministic title. The task never blocks the send path and never
//!   enters the query event stream.
//!
//! ## Manual rename wins (the red line)
//!
//! No new on-disk marker: the guard piggybacks on the Tier-1 placeholder
//! rule. The LLM task is spawned **only** when `auto_title_from_first_message`
//! actually retitled the session (placeholder → derived truncation), and the
//! apply step re-validates against that captured title (compare-and-swap) —
//! so a user rename in the async window is discarded, a session renamed
//! before its first send never spawns a task at all (the placeholder guard
//! skips the auto-title, and retitling is this module's only trigger), and a
//! session renamed in a previous process is covered by the same rule.
//! Verified by the tests below.
//!
//! ## One attempt, no retry
//!
//! Every session gets at most ONE LLM title attempt per process (the
//! `ATTEMPTED` set, marked only when an attempt actually launches). A failed
//! attempt keeps the deterministic title forever — this is not a retry loop.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use shannon_types::provider_config::AuxRole;

use crate::commands::{AppState, ChatMessage};
use crate::events::event_names;

/// Hard cap on a generated title, matching `derive_title_from_message`'s
/// `MAX_CHARS` so a slot-generated title renders on the rail exactly like
/// the deterministic one.
const TITLE_MAX_CHARS: usize = 50;
/// Truncation budget per prompt input (first user message / first assistant
/// reply). A title needs the topic, not the transcript.
const INPUT_MAX_CHARS: usize = 400;
/// A title never needs more than a few tokens; 32 leaves headroom for
/// verbose tokenizers without letting a runaway model bill a paragraph.
const TITLE_MAX_TOKENS: u32 = 32;
/// Total request deadline (`LlmClientConfig::timeout_seconds` — the
/// non-streaming path applies it as a whole-request timeout). Past this the
/// call errors and the deterministic title stays.
const TITLE_TIMEOUT_SECS: u64 = 5;

/// Sessions whose one LLM title attempt already launched this process.
/// Process-global (AppState is a process singleton; a module global keeps
/// the AppState surface untouched and the tests hermetic — every test drives
/// a fresh UUID, so the set is collision-free by construction).
static ATTEMPTED: LazyLock<Mutex<HashSet<uuid::Uuid>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark `session_id` attempted; `false` = an attempt already launched (the
/// caller must not retry).
fn mark_attempted(session_id: uuid::Uuid) -> bool {
    ATTEMPTED
        .lock()
        .expect("session title attempt set poisoned")
        .insert(session_id)
}

/// Boundary-safe truncation for prompt inputs (no ellipsis — the model only
/// needs the gist, and a marker could leak into generated titles).
fn truncate_chars(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

/// Reduce a model reply to a single-line rail-safe title. Strips one pair of
/// wrapping quotes (the model's most common decoration), takes the first
/// line, caps at [`TITLE_MAX_CHARS`]. `None` = nothing usable (empty, or no
/// alphanumeric/ideographic content at all — `""`, `"???"`, a bare dash) —
/// the caller keeps the deterministic title.
pub(crate) fn sanitize_generated_title(raw: &str) -> Option<String> {
    let first_line = raw.lines().next().unwrap_or("").trim();
    let title = strip_wrapping_quotes(first_line);
    if title.is_empty() || !title.chars().any(|c| c.is_alphanumeric()) {
        return None;
    }
    if title.chars().count() <= TITLE_MAX_CHARS {
        Some(title.to_string())
    } else {
        let truncated: String = title.chars().take(TITLE_MAX_CHARS).collect();
        Some(format!("{truncated}…"))
    }
}

/// Strip ONE pair of wrapping quotes (`"…"`, `'…'`, `“…”`, `‘…’`) plus the
/// whitespace around what's inside.
fn strip_wrapping_quotes(s: &str) -> &str {
    const PAIRS: [(&str, &str); 4] = [("\"", "\""), ("'", "'"), ("“", "”"), ("‘", "’")];
    let t = s.trim();
    for (open, close) in PAIRS {
        if t.len() >= open.len() + close.len() && t.starts_with(open) && t.ends_with(close) {
            return t[open.len()..t.len() - close.len()].trim();
        }
    }
    t
}

/// The system side of the title request: short title, conversation's
/// language, nothing but the title.
pub(crate) fn title_system_prompt() -> &'static str {
    "You write concise conversation titles. Reply with ONLY the title text: \
     4-12 words (or 4-12 characters for CJK), in the same language as the \
     conversation. No quotes, no surrounding punctuation, no explanation."
}

/// The user side of the title request: the first exchange, truncated.
/// A missing/empty assistant reply is omitted (a failed first turn still
/// gets a title, from the user message alone).
pub(crate) fn title_user_prompt(user_message: &str, assistant_reply: Option<&str>) -> String {
    let mut prompt = String::from("Write a short title for this conversation.\n\nUser: ");
    prompt.push_str(&truncate_chars(user_message, INPUT_MAX_CHARS));
    prompt.push('\n');
    if let Some(reply) = assistant_reply
        .map(|r| truncate_chars(r, INPUT_MAX_CHARS))
        .filter(|r| !r.trim().is_empty())
    {
        prompt.push_str("Assistant: ");
        prompt.push_str(&reply);
        prompt.push('\n');
    }
    prompt.push_str("\nTitle:");
    prompt
}

/// The first exchange of a settled first query: the first user message plus
/// the first assistant reply after it (empty replies normalized to `None`).
/// `None` = no user message at all (nothing to title from).
pub(crate) fn first_exchange(messages: &[ChatMessage]) -> Option<(String, Option<String>)> {
    let user_idx = messages.iter().position(|m| m.role == "user")?;
    let user = messages[user_idx].content.clone();
    let assistant = messages[user_idx + 1..]
        .iter()
        .find(|m| m.role == "assistant")
        .map(|m| m.content.clone())
        .filter(|c| !c.trim().is_empty());
    Some((user, assistant))
}

/// The send path's hook, called from `send_message`'s settle block (after
/// the per-session flags reset, on every exit path). Spawns the
/// fire-and-forget title task; the caller has already gated on
/// `auto_title_from_first_message` returning `Some` (see the module docs —
/// that `Some` IS the manual-rename guard).
pub(crate) fn spawn_title_task<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: uuid::Uuid,
    expected_title: String,
    exchange: Option<(String, Option<String>)>,
) {
    tokio::spawn(async move {
        run_title_task(&app, session_id, &expected_title, exchange).await;
    });
}

/// The title task body (spawned by [`spawn_title_task`], awaited directly by
/// the tests). Resolves the slot, makes the ONE small request, applies the
/// title through the rename persistence path — every failure mode degrades
/// to "keep the deterministic title".
pub(crate) async fn run_title_task<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: uuid::Uuid,
    expected_title: &str,
    exchange: Option<(String, Option<String>)>,
) {
    use tauri::Manager;

    let Some((user_text, assistant_text)) = exchange else {
        return;
    };
    if user_text.trim().is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    // Slot target or bust: unconfigured (the default) exits silently BEFORE
    // the attempt is marked — configuring the slot later still lets this
    // session's first-query settle get its LLM title (no attempt burned).
    let Some(mut client_config) =
        crate::utility_tier::resolve_auxiliary_client_config(&state, AuxRole::TitleGeneration)
            .await
    else {
        return;
    };
    if !mark_attempted(session_id) {
        tracing::debug!(
            session = %session_id,
            "LLM title already attempted for this session — keeping the current title"
        );
        return;
    }
    // The one background request's budget: tiny response, hard 5s deadline,
    // no thinking (GLM-family servers default thinking ON, which would eat
    // the 32-token budget; the toggle is stripped on wires that reject it).
    client_config.max_tokens = TITLE_MAX_TOKENS;
    client_config.timeout_seconds = TITLE_TIMEOUT_SECS;
    client_config.thinking_type = Some("disabled".to_string());
    let client = shannon_engine::api::client::LlmClient::new(client_config);

    match generate_title(&client, &user_text, assistant_text.as_deref()).await {
        Ok(title) => {
            if apply_generated_title(&state, app, session_id, expected_title, title).await {
                tracing::info!(
                    session = %session_id,
                    "LLM session title applied (TitleGeneration utility slot)"
                );
            } else {
                tracing::debug!(
                    session = %session_id,
                    "LLM title discarded — the session title changed while generating \
                     (user rename wins)"
                );
            }
        }
        Err(e) => tracing::warn!(
            error = %e,
            session = %session_id,
            "LLM title generation failed — keeping the deterministic title"
        ),
    }
}

/// Apply a generated title through the Tier-1 persistence path (sessions
/// vec + sidecar merge + `sessions:updated`), guarded by a compare-and-swap
/// on `expected_title`: the deterministic title captured at send time must
/// still be current. A user rename between send and apply changes the
/// title, the CAS fails, and the generated title is discarded — the user's
/// name is never overwritten, no matter how slow the slot target is.
/// Returns whether the title was applied.
pub(crate) async fn apply_generated_title<R: tauri::Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    session_id: uuid::Uuid,
    expected_title: &str,
    new_title: String,
) -> bool {
    use tauri::Emitter;

    let id_str = session_id.to_string();
    {
        let mut sessions = state.sessions.lock().await;
        let Some(session) = sessions.iter_mut().find(|s| s.id == id_str) else {
            return false;
        };
        if session.title != expected_title {
            return false;
        }
        session.title = new_title.clone();
    }

    // Merge-persist (goal/loop/ralph sidecar fields survive), mirroring the
    // Tier-1 auto-title write.
    let _ = state.l0_store().save_sidecar(
        &session_id,
        &shannon_core::session_log::SessionSidecar {
            title: Some(new_title),
            ..Default::default()
        },
    );

    let _ = app.emit(event_names::SESSIONS_UPDATED, ());
    true
}

/// The single small chat request. `Err` on any transport/HTTP/parse failure
/// (the caller warns and keeps the deterministic title).
async fn generate_title(
    client: &shannon_engine::api::client::LlmClient,
    user_message: &str,
    assistant_reply: Option<&str>,
) -> Result<String, shannon_engine::api::ApiError> {
    let messages = vec![shannon_engine::api::types::Message {
        role: "user".to_string(),
        content: shannon_engine::api::types::MessageContent::Text(title_user_prompt(
            user_message,
            assistant_reply,
        )),
    }];
    let blocks = client
        .send_message(messages, None, Some(title_system_prompt().to_string()))
        .await?;
    let mut text = String::new();
    for block in blocks {
        if let shannon_engine::api::types::ContentBlock::Text { text: chunk } = block {
            text.push_str(&chunk);
        }
    }
    sanitize_generated_title(&text).ok_or_else(|| {
        shannon_engine::api::ApiError::InvalidResponse(
            "model returned no usable title text".to_string(),
        )
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::SessionMeta;
    use axum::Json;
    use shannon_core::provider_config_store::ProviderConfigStore;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderProfile,
        ProviderTiers, Scope,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tauri::Manager;

    type MockApp = tauri::AppHandle<tauri::test::MockRuntime>;

    /// MockRuntime app + AppState with the sessions dir (and therefore the
    /// L0 sidecar store) redirected into a tempdir. `AppState::new()` only
    /// reads ambient config — nothing here writes outside the tempdir.
    fn mock_app() -> (MockApp, tempfile::TempDir) {
        let app = tauri::test::mock_app().handle().clone();
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new();
        state.state_manager = Arc::new(
            shannon_engine::state::StateManager::with_sessions_dir(dir.path().join("sessions"))
                .unwrap(),
        );
        app.manage(state);
        (app, dir)
    }

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: 0,
            file_attachments: None,
            interrupted: None,
            interrupted_reason: None,
        }
    }

    async fn seed_session(state: &AppState, title: &str) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        state.sessions.lock().await.push(SessionMeta {
            id: id.to_string(),
            title: title.to_string(),
            created_at: 0,
            message_count: 0,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        });
        id
    }

    async fn session_title(state: &AppState, id: uuid::Uuid) -> String {
        let id_str = id.to_string();
        state
            .sessions
            .lock()
            .await
            .iter()
            .find(|s| s.id == id_str)
            .map(|s| s.title.clone())
            .unwrap_or_default()
    }

    /// A providers.toml snapshot whose active profile hosts ONE
    /// OpenAI-compatible roster slot (`mock-utility` at `base_url`) and,
    /// when `with_slot`, a TitleGeneration auxiliary target pinned to it
    /// with model `title-model-1`.
    fn fixture_config(
        base_url: &str,
        with_slot: bool,
    ) -> shannon_types::provider_config::ProviderModelConfig {
        use shannon_types::provider_config::ProviderModelConfig;

        let mock = ProviderProfile {
            id: "mock-utility".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "Mock".to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::InlineLegacy {
                masked: "test-key".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        };
        let mut auxiliary = HashMap::new();
        if with_slot {
            auxiliary.insert(
                AuxRole::TitleGeneration,
                ActiveTarget {
                    provider_id: "mock-utility".to_string(),
                    model_id: "title-model-1".to_string(),
                    scope: Scope::Global,
                },
            );
        }
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "mock-utility".to_string(),
                    model_id: "main-model".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![mock],
                auxiliary,
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles,
            gateway: Default::default(),
        }
    }

    /// Install `config` as the app's provider store.
    async fn install_config(
        app: &MockApp,
        config: shannon_types::provider_config::ProviderModelConfig,
    ) {
        let state = app.state::<AppState>();
        let mut store = state.provider_store.lock().await;
        *store = ProviderConfigStore::from_config(config);
    }

    /// Minimal OpenAI-compatible `/v1/chat/completions` double: records
    /// every request body, replies `status` with `content` as the message
    /// text. Returns the base URL and the capture buffer.
    async fn spawn_mock_title_endpoint(
        status: axum::http::StatusCode,
        content: &'static str,
    ) -> (String, Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
        let captured: Arc<std::sync::Mutex<Vec<serde_json::Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let handler_state = captured.clone();

        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |Json(body): Json<serde_json::Value>| async move {
                handler_state.lock().unwrap().push(body);
                (
                    status,
                    Json(serde_json::json!({
                        "id": "chatcmpl-title",
                        "object": "chat.completion",
                        "model": "mock",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": content},
                            "finish_reason": "stop"
                        }]
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock");
        });
        (format!("http://{addr}"), captured)
    }

    // ── pure helpers ───────────────────────────────────────────────────

    #[test]
    fn sanitize_takes_first_line_and_strips_wrapping_quotes() {
        assert_eq!(
            sanitize_generated_title("\"Fix the login bug\""),
            Some("Fix the login bug".to_string())
        );
        assert_eq!(
            sanitize_generated_title("  “规划一次东京旅行” \n附注行"),
            Some("规划一次东京旅行".to_string())
        );
        assert_eq!(
            sanitize_generated_title("Plan a trip\n\nHere is why:"),
            Some("Plan a trip".to_string())
        );
        // No quotes → untouched; inner quotes survive.
        assert_eq!(
            sanitize_generated_title("Fix the \"login\" bug"),
            Some("Fix the \"login\" bug".to_string())
        );
        assert_eq!(sanitize_generated_title("   "), None);
        assert_eq!(sanitize_generated_title("\"\""), None);
        assert_eq!(sanitize_generated_title(""), None);
    }

    #[test]
    fn sanitize_caps_at_the_deterministic_rail_cap() {
        let long = "x".repeat(120);
        let title = sanitize_generated_title(&long).unwrap();
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS + 1); // + ellipsis
        assert!(title.ends_with('…'));

        let zh = "标".repeat(80);
        assert_eq!(
            sanitize_generated_title(&zh).unwrap().chars().count(),
            TITLE_MAX_CHARS + 1
        );
    }

    #[test]
    fn user_prompt_carries_truncated_inputs_and_skips_empty_replies() {
        let prompt =
            title_user_prompt("Help me plan a trip to Tokyo", Some("Sure — here's a plan"));
        assert!(prompt.contains("User: Help me plan a trip to Tokyo"));
        assert!(prompt.contains("Assistant: Sure — here's a plan"));
        assert!(prompt.ends_with("\nTitle:"));

        // Empty/whitespace assistant reply → omitted, not "Assistant: ".
        let prompt = title_user_prompt("Hello", Some("   \n"));
        assert!(prompt.contains("User: Hello"));
        assert!(!prompt.contains("Assistant:"));

        // Inputs longer than the budget are char-boundary-safe-truncated.
        let long_user = "日".repeat(INPUT_MAX_CHARS + 50);
        let prompt = title_user_prompt(&long_user, None);
        assert!(prompt.contains(&"日".repeat(INPUT_MAX_CHARS)));
        assert!(!prompt.contains(&"日".repeat(INPUT_MAX_CHARS + 1)));
    }

    #[test]
    fn first_exchange_extracts_the_first_pair() {
        let buffer = vec![
            msg("user", "first question"),
            msg("assistant", "first answer"),
            msg("user", "second question"),
            msg("assistant", "second answer"),
        ];
        assert_eq!(
            first_exchange(&buffer),
            Some((
                "first question".to_string(),
                Some("first answer".to_string())
            ))
        );

        // Failed first turn: no usable assistant reply → None reply.
        let failed = vec![msg("user", "only the question")];
        assert_eq!(
            first_exchange(&failed),
            Some(("only the question".to_string(), None))
        );
        let blank_reply = vec![msg("user", "q"), msg("assistant", "  ")];
        assert_eq!(first_exchange(&blank_reply), Some(("q".to_string(), None)));

        // No user message → nothing to title from.
        assert_eq!(first_exchange(&[msg("assistant", "hi")]), None);
        assert_eq!(first_exchange(&[]), None);
    }

    #[test]
    fn attempt_marking_is_once_per_session() {
        let id = uuid::Uuid::new_v4();
        assert!(mark_attempted(id), "first mark goes through");
        assert!(!mark_attempted(id), "second mark is the no-retry signal");
    }

    // ── slot-driven integration (mock OpenAI-compatible endpoint) ──────

    #[tokio::test]
    async fn unconfigured_slot_generates_nothing_and_marks_no_attempt() {
        let (app, _dir) = mock_app();
        install_config(&app, fixture_config("http://127.0.0.1:1", false)).await;
        let state = app.state::<AppState>();
        let session_id = seed_session(&state, "Help me plan a trip to To…").await;

        run_title_task(
            &app,
            session_id,
            "Help me plan a trip to To…",
            Some(("Help me plan a trip".to_string(), Some("Sure".to_string()))),
        )
        .await;

        assert_eq!(
            session_title(&state, session_id).await,
            "Help me plan a trip to To…"
        );
        // No attempt was burned: a slot configured afterwards still gets its
        // shot (pinned by the configured-slot test below using a fresh id).
    }

    #[tokio::test]
    async fn configured_slot_titles_via_the_slot_target_and_persists() {
        let (base_url, captured) =
            spawn_mock_title_endpoint(axum::http::StatusCode::OK, " \"Plan a Tokyo trip\" ").await;
        let (app, _dir) = mock_app();
        install_config(&app, fixture_config(&base_url, true)).await;
        let state = app.state::<AppState>();
        let session_id = seed_session(&state, "Help me plan a trip to To…").await;

        run_title_task(
            &app,
            session_id,
            "Help me plan a trip to To…",
            Some((
                "Help me plan a trip to Tokyo next spring".to_string(),
                Some("Sure — flights, hotels, and a rail pass.".to_string()),
            )),
        )
        .await;

        // Title applied (mock replies with a quoted title → quotes stripped).
        assert_eq!(session_title(&state, session_id).await, "Plan a Tokyo trip");
        // Persisted through the sidecar like a user rename.
        assert_eq!(
            state.l0_store().sidecar(&session_id).title.as_deref(),
            Some("Plan a Tokyo trip")
        );

        // The request went to the SLOT target with the one-small-request
        // budget — model from the auxiliary target, token cap on the wire
        // (the OpenAI wire emits the cap as `max_completion_tokens`).
        // Scoped block: the guard must drop before the `.await` below.
        {
            let bodies = captured.lock().unwrap();
            assert_eq!(bodies.len(), 1, "exactly one small request");
            let body = &bodies[0];
            assert_eq!(body["model"], "title-model-1");
            assert_eq!(body["max_completion_tokens"], TITLE_MAX_TOKENS);
            // Wire shape: system prompt + ONE user message carrying the
            // truncated first exchange.
            let messages = body["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(
                messages[0]["role"], "system",
                "the title system prompt rides the system field... as the wire's system message"
            );
            assert!(
                messages[0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("concise conversation titles")
            );
            assert_eq!(messages[1]["role"], "user");
            let prompt = messages[1]["content"].as_str().unwrap();
            assert!(prompt.contains("Help me plan a trip to Tokyo next spring"));
            assert!(prompt.contains("Sure — flights, hotels, and a rail pass."));
        }

        // One attempt per session: a second settled trigger does not retry.
        run_title_task(
            &app,
            session_id,
            "Plan a Tokyo trip",
            Some(("Help me plan a trip".to_string(), None)),
        )
        .await;
        assert_eq!(captured.lock().unwrap().len(), 1, "no second attempt");
    }

    #[tokio::test]
    async fn llm_failure_keeps_the_deterministic_title() {
        let (base_url, captured) =
            spawn_mock_title_endpoint(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "").await;
        let (app, _dir) = mock_app();
        install_config(&app, fixture_config(&base_url, true)).await;
        let state = app.state::<AppState>();
        let session_id = seed_session(&state, "Fix the login bug").await;

        run_title_task(
            &app,
            session_id,
            "Fix the login bug",
            Some(("Fix the login bug".to_string(), Some("On it.".to_string()))),
        )
        .await;

        assert_eq!(
            session_title(&state, session_id).await,
            "Fix the login bug",
            "a failed slot target silently keeps the deterministic title"
        );
        assert_eq!(
            state.l0_store().sidecar(&session_id).title,
            None,
            "no sidecar write on failure"
        );
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unusable_reply_text_keeps_the_deterministic_title() {
        // Whitespace-only model output sanitizes to None → same fallback.
        let (base_url, _) = spawn_mock_title_endpoint(axum::http::StatusCode::OK, "   ").await;
        let (app, _dir) = mock_app();
        install_config(&app, fixture_config(&base_url, true)).await;
        let state = app.state::<AppState>();
        let session_id = seed_session(&state, "Fix the login bug").await;

        run_title_task(
            &app,
            session_id,
            "Fix the login bug",
            Some(("Fix the login bug".to_string(), None)),
        )
        .await;

        assert_eq!(session_title(&state, session_id).await, "Fix the login bug");
    }

    #[tokio::test]
    async fn apply_compare_and_swap_never_overwrites_a_user_rename() {
        let (app, _dir) = mock_app();
        let state = app.state::<AppState>();

        // Normal path: the captured deterministic title is still current →
        // applied through the rename persistence path.
        let applied_id = seed_session(&state, "Derived Title").await;
        let applied = apply_generated_title(
            &state,
            &app,
            applied_id,
            "Derived Title",
            "LLM Title".to_string(),
        )
        .await;
        assert!(applied);
        assert_eq!(session_title(&state, applied_id).await, "LLM Title");
        assert_eq!(
            state.l0_store().sidecar(&applied_id).title.as_deref(),
            Some("LLM Title")
        );

        // The red line: the user renamed while the title was generating →
        // the CAS misses → the generated title is discarded.
        let renamed_id = seed_session(&state, "Derived Title").await;
        {
            let id_str = renamed_id.to_string();
            let mut sessions = state.sessions.lock().await;
            sessions.iter_mut().find(|s| s.id == id_str).unwrap().title = "My Own Name".to_string();
        }
        let applied = apply_generated_title(
            &state,
            &app,
            renamed_id,
            "Derived Title",
            "LLM Title".to_string(),
        )
        .await;
        assert!(!applied);
        assert_eq!(
            session_title(&state, renamed_id).await,
            "My Own Name",
            "a user rename between send and apply is never overwritten"
        );
        assert_eq!(state.l0_store().sidecar(&renamed_id).title, None);

        // Unknown session → nothing to apply.
        let applied = apply_generated_title(
            &state,
            &app,
            uuid::Uuid::new_v4(),
            "Derived Title",
            "LLM Title".to_string(),
        )
        .await;
        assert!(!applied);
    }
}
