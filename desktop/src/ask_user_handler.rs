//! Desktop ask_user question handler — the GUI replacement for the terminal
//! stdin `QuestionHandler` (`shannon_tools::ask_user::QuestionHandler`)
//! (Settings R3 T8, C3 + R8).
//!
//! `register_default_tools_with_providers` registers `ask_user_question`
//! backed by `TerminalQuestionHandler`, which blocks on stdin — unusable
//! under a GUI (EOF → `AskUserError::NoInput`, or a hung read). This module
//! swaps the same-named tool for one backed by `DesktopQuestionHandler`:
//!
//! 1. `ask_question` mints a `request_id`, parks a oneshot sender in
//!    `AppState::pending_questions` and emits `ask-user-request`.
//! 2. The frontend's AskUserCard renders the question; the answer travels
//!    back through the [`respond_ask_user`] command (remove + send —
//!    repeating `commands_permissions::respond_permission`'s contract,
//!    but idempotent: an unknown/expired id is a logged `Ok`).
//! 3. With 提问自动继续 (`chat_ask_user_auto_continue`, default off) the
//!    wait is bounded by [`ASK_USER_TIMEOUT_SECS`]; on timeout the pending
//!    entry is dropped, an `ask-user-resolved` (timed_out) event is emitted
//!    and the tool receives a best-judgment continuation answer — mirroring
//!    the permission side's timeout auto-deny
//!    (`commands_permissions::prompt_user`). Off → wait forever.

use crate::commands::AppState;
use crate::events::{self, event_names};
use shannon_tools::ToolResult;
use shannon_tools::ask_user::{AskUserError, Question, QuestionHandler};
use shannon_tools::{AskUserQuestionTool, ToolRegistry};
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

/// Tool name being overridden — must equal `AskUserQuestionTool::name()`
/// (shannon-tools registers it under this exact string).
const ASK_USER_TOOL_NAME: &str = "ask_user_question";

/// 提问自动继续 window: an unanswered question is auto-answered after 5
/// minutes (the countdown the AskUserCard renders is pure display — this
/// backend timeout is the authority).
pub const ASK_USER_TIMEOUT_SECS: u64 = 300;

/// The preset answer fed to the model when the user does not answer in
/// time — a continuation instruction, not an error (the run proceeds).
pub(crate) const AUTO_CONTINUE_ANSWER: &str = "(用户未回答,请按你的最佳判断继续)";

/// `QuestionHandler` backed by the desktop UI. Generic over the Tauri
/// runtime so tests drive it against `tauri::test::MockRuntime`; production
/// always passes `AppHandle` (`Wry`).
pub(crate) struct DesktopQuestionHandler<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    /// Auto-continue window in seconds, applied only when
    /// `chat_ask_user_auto_continue` is on (read live from the managed
    /// [`AppState`] config before each wait, so a settings flip applies to
    /// the NEXT question without a restart). A field — not a constant — so
    /// tests inject a 1s-scale timeout; production passes
    /// [`ASK_USER_TIMEOUT_SECS`].
    timeout_secs: u64,
}

impl<R: tauri::Runtime> DesktopQuestionHandler<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>, timeout_secs: u64) -> Self {
        Self { app, timeout_secs }
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> QuestionHandler for DesktopQuestionHandler<R> {
    async fn ask_question(&self, question: &Question) -> Result<Vec<String>, AskUserError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Read the auto-continue switch live and park the oneshot. The
        // DashMap critical sections are pure map edits — never held across
        // the await below.
        let timeout_ms = {
            let state = self.app.state::<AppState>();
            let auto_continue = state
                .desktop_config
                .read()
                .await
                .chat_ask_user_auto_continue;
            state.pending_questions.insert(request_id.clone(), tx);
            if auto_continue {
                Some(self.timeout_secs.saturating_mul(1000))
            } else {
                None // 无限等待 — the user answers whenever they answer.
            }
        };

        let _ = self.app.emit(
            event_names::ASK_USER_REQUEST,
            events::AskUserRequest {
                request_id: request_id.clone(),
                question: question.question.clone(),
                header: question.header.clone(),
                options: question
                    .options
                    .iter()
                    .map(|o| events::AskUserOptionPayload {
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect(),
                multi_select: question.multi_select,
                timeout_ms,
            },
        );

        let wait = async {
            match rx.await {
                Ok(answers) => Ok(answers),
                // Responder gone without answering (app shutting down) —
                // the same degradation the terminal handler reports on EOF.
                Err(_) => Err(AskUserError::NoInput),
            }
        };

        match timeout_ms {
            Some(ms) => {
                match tokio::time::timeout(std::time::Duration::from_millis(ms), wait).await {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        // Drop the stale pending entry so a late answer is a
                        // no-op (respond_ask_user's idempotent None branch),
                        // then tell every window the question timed out.
                        self.app
                            .state::<AppState>()
                            .pending_questions
                            .remove(&request_id);
                        let _ = self.app.emit(
                            event_names::ASK_USER_RESOLVED,
                            events::AskUserResolved {
                                request_id: request_id.clone(),
                                timed_out: true,
                            },
                        );
                        tracing::info!(
                            %request_id,
                            timeout_secs = self.timeout_secs,
                            "ask_user unanswered — auto-continuing with best-judgment answer"
                        );
                        Ok(vec![AUTO_CONTINUE_ANSWER.to_string()])
                    }
                }
            }
            None => wait.await,
        }
    }
}

/// Registry-level override: unregister the terminal stdin
/// `ask_user_question`, then register the desktop event round-trip under
/// the same name. `ToolRegistry::register` rejects duplicates by design and
/// that contract stays untouched — `unregister` already performs the
/// cache invalidation + deferred-set cleanup a `register_override` method
/// would redo, so composing the two is the minimal-intrusion path (no
/// shared-crate change at all).
///
/// A missing predecessor is ignored (`NotFound`): a future shannon-tools
/// that stops registering the terminal handler must not break startup.
pub(crate) fn register_into_registry<R: tauri::Runtime>(
    tools: &ToolRegistry,
    app: tauri::AppHandle<R>,
) -> ToolResult<()> {
    let _ = tools.unregister(ASK_USER_TOOL_NAME);
    tools.register(Box::new(AskUserQuestionTool::new(std::sync::Arc::new(
        DesktopQuestionHandler::new(app, ASK_USER_TIMEOUT_SECS),
    ))))
}

/// `AppState`-level wrapper for `main.rs` setup (the bin crate cannot reach
/// the `pub(crate)` `tools` field itself — same `pub` seam as
/// `skill_tools::register_for_state`). Called once after
/// `register_default_tools_with_providers`, before the first query can run.
pub fn register_for_state<R: tauri::Runtime>(
    state: &AppState,
    app: tauri::AppHandle<R>,
) -> ToolResult<()> {
    register_into_registry(&state.tools, app)
}

/// Answer a pending `ask-user-request`. The pending entry is removed before
/// the answer is delivered, so a second submit for the same `request_id`
/// hits the `None` branch: logged, `Ok` — idempotent. Unknown ids (already
/// timed out, stale window, stray call) are the same silent `Ok` rather
/// than an error the UI would have to special-case.
#[tauri::command]
pub async fn respond_ask_user(
    state: tauri::State<'_, AppState>,
    request_id: String,
    answers: Vec<String>,
) -> Result<(), String> {
    match state.pending_questions.remove(&request_id) {
        Some((_, tx)) => {
            tracing::debug!(%request_id, answers = answers.len(), "ask_user answered");
            // Receiver gone (the timeout raced this click) — the answer is
            // simply dropped; the tool already continued.
            let _ = tx.send(answers);
            Ok(())
        }
        None => {
            tracing::debug!(
                %request_id,
                "respond_ask_user for unknown/expired request — ignoring (idempotent)"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_tools::ask_user::QuestionOption;

    /// Mock-runtime app with a managed [`AppState`]: the production shape
    /// (`ask_question` resolves state through `app.state::<AppState>()`).
    fn mock_app_with_state() -> (
        tauri::AppHandle<tauri::test::MockRuntime>,
        tauri::App<tauri::test::MockRuntime>,
    ) {
        let app = tauri::test::mock_app();
        app.manage(AppState::new());
        (app.handle().clone(), app)
    }

    fn sample_question() -> Question {
        Question {
            question: "Deploy to production?".to_string(),
            header: "Confirm".to_string(),
            options: vec![
                QuestionOption {
                    label: "Yes".to_string(),
                    description: "Ship it".to_string(),
                },
                QuestionOption {
                    label: "No".to_string(),
                    description: String::new(),
                },
            ],
            multi_select: false,
        }
    }

    /// Wait until exactly one pending question is registered, return its id.
    async fn wait_pending<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
        for _ in 0..200 {
            {
                let state = app.state::<AppState>();
                if let Some(entry) = state.pending_questions.iter().next() {
                    return entry.key().clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("no pending ask_user question appeared");
    }

    async fn set_auto_continue<R: tauri::Runtime>(app: &tauri::AppHandle<R>, on: bool) {
        let state = app.state::<AppState>();
        state
            .desktop_config
            .write()
            .await
            .chat_ask_user_auto_continue = on;
    }

    #[tokio::test]
    async fn answered_question_returns_the_submitted_answers() {
        let (handle, _app) = mock_app_with_state();
        let handler = DesktopQuestionHandler::new(handle.clone(), 60);
        set_auto_continue(&handle, true).await; // timeout armed but the answer wins the race

        let q = sample_question();
        let ask = tokio::spawn(async move { handler.ask_question(&q).await });
        let request_id = wait_pending(&handle).await;

        // Answer through the command body — the exact path `respond_ask_user`
        // runs in production.
        respond_ask_user(handle.state(), request_id.clone(), vec!["Yes".to_string()])
            .await
            .unwrap();

        let answers = ask.await.unwrap().unwrap();
        assert_eq!(answers, vec!["Yes".to_string()]);
        // Answered → the pending entry is gone (remove semantics).
        {
            let state = handle.state::<AppState>();
            assert!(!state.pending_questions.contains_key(&request_id));
        }
    }

    #[tokio::test]
    async fn duplicate_and_unknown_responses_are_idempotent_ok() {
        let (handle, _app) = mock_app_with_state();
        let handler = DesktopQuestionHandler::new(handle.clone(), 60);

        let q = sample_question();
        let ask = tokio::spawn(async move { handler.ask_question(&q).await });
        let request_id = wait_pending(&handle).await;

        respond_ask_user(handle.state(), request_id.clone(), vec!["No".to_string()])
            .await
            .unwrap();
        // Second click / stale window: no entry → logged Ok, never an error.
        respond_ask_user(handle.state(), request_id.clone(), vec!["Yes".to_string()])
            .await
            .unwrap();
        // Fully unknown id: same idempotent Ok.
        respond_ask_user(
            handle.state(),
            "ghost-id".to_string(),
            vec!["x".to_string()],
        )
        .await
        .unwrap();

        // The FIRST answer won (the entry was removed before the replay).
        assert_eq!(ask.await.unwrap().unwrap(), vec!["No".to_string()]);
    }

    #[tokio::test]
    async fn timeout_answers_with_preset_text_resolves_pending_and_emits_event() {
        let (handle, _app) = mock_app_with_state();
        set_auto_continue(&handle, true).await;

        let resolved: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        {
            let resolved = resolved.clone();
            use tauri::Listener;
            handle.listen(event_names::ASK_USER_RESOLVED, move |event| {
                resolved
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(event.payload().to_string());
            });
        }

        let handler = DesktopQuestionHandler::new(handle.clone(), 1); // 1s — test-scale
        let q = sample_question();
        let ask = tokio::spawn(async move { handler.ask_question(&q).await });
        let request_id = wait_pending(&handle).await;

        // Do NOT answer — the auto-continue path must answer for the user.
        let answers = tokio::time::timeout(std::time::Duration::from_secs(5), ask)
            .await
            .expect("timeout path did not resolve the ask")
            .unwrap()
            .unwrap();
        assert_eq!(answers, vec![AUTO_CONTINUE_ANSWER.to_string()]);

        // Pending entry dropped → a late answer is an idempotent no-op.
        {
            let state = handle.state::<AppState>();
            assert!(!state.pending_questions.contains_key(&request_id));
        }
        respond_ask_user(handle.state(), request_id.clone(), vec!["late".to_string()])
            .await
            .unwrap();

        // The card's timeout cue: exactly one resolved event, this id,
        // timed_out=true.
        let events = resolved.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert_eq!(events.len(), 1, "exactly one resolved event: {events:?}");
        let payload: serde_json::Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(payload["request_id"], request_id);
        assert_eq!(payload["timed_out"], true);
    }

    #[tokio::test]
    async fn switch_off_waits_indefinitely_and_answers_late_without_timeout() {
        let (handle, _app) = mock_app_with_state();
        set_auto_continue(&handle, false).await; // default posture — wait forever

        let resolved: std::sync::Arc<std::sync::Mutex<usize>> = Default::default();
        {
            let resolved = resolved.clone();
            use tauri::Listener;
            handle.listen(event_names::ASK_USER_RESOLVED, move |_| {
                *resolved.lock().unwrap_or_else(|p| p.into_inner()) += 1;
            });
        }

        // The injected timeout would have fired at 1s — answering AFTER that
        // boundary proves the switch-off path never times out.
        let handler = DesktopQuestionHandler::new(handle.clone(), 1);
        let q = sample_question();
        let ask = tokio::spawn(async move { handler.ask_question(&q).await });
        let request_id = wait_pending(&handle).await;

        tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
        respond_ask_user(
            handle.state(),
            request_id.clone(),
            vec!["finally".to_string()],
        )
        .await
        .unwrap();

        let answers = tokio::time::timeout(std::time::Duration::from_secs(5), ask)
            .await
            .expect("switch-off path timed out — it must wait indefinitely")
            .unwrap()
            .unwrap();
        assert_eq!(answers, vec!["finally".to_string()]);

        // No auto-continue → no resolved event, ever.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(*resolved.lock().unwrap_or_else(|p| p.into_inner()), 0);
    }

    #[tokio::test]
    async fn responder_dropped_without_answer_maps_to_no_input() {
        let (handle, _app) = mock_app_with_state();

        let handler = DesktopQuestionHandler::new(handle.clone(), 60);
        let q = sample_question();
        let ask = tokio::spawn(async move { handler.ask_question(&q).await });
        let request_id = wait_pending(&handle).await;

        // Simulate the app tearing down the pending map without an answer.
        handle
            .state::<AppState>()
            .pending_questions
            .remove(&request_id);

        let result = ask.await.unwrap();
        assert!(matches!(result, Err(AskUserError::NoInput)));
    }

    #[tokio::test]
    async fn registry_override_replaces_terminal_tool_and_routes_through_desktop_handler() {
        let (handle, _app) = mock_app_with_state();

        let registry = ToolRegistry::new();
        registry
            .register(Box::new(AskUserQuestionTool::with_terminal_handler()))
            .unwrap();

        register_into_registry(&registry, handle.clone()).unwrap();

        // Still exactly one ask_user_question tool…
        let infos = registry.list_tools_info();
        let matches = infos
            .iter()
            .filter(|t| t.name == ASK_USER_TOOL_NAME)
            .count();
        assert_eq!(matches, 1, "override must replace, not duplicate");

        // …and executing it goes through the DESKTOP handler (the question
        // lands in pending_questions — the terminal handler would block on
        // stdin instead).
        let input = serde_json::json!({
            "questions": [{
                "question": "Registry override check?",
                "options": [{"label": "Ok"}]
            }]
        });
        let exec = tokio::spawn(async move { registry.execute(ASK_USER_TOOL_NAME, input).await });
        let request_id = wait_pending(&handle).await;
        respond_ask_user(handle.state(), request_id, vec!["Ok".to_string()])
            .await
            .unwrap();

        let output = tokio::time::timeout(std::time::Duration::from_secs(5), exec)
            .await
            .expect("override tool did not resolve")
            .unwrap()
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Registry override check?"));
        assert!(output.content.contains("Ok"));
    }
}
