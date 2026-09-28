use axum::response::sse::Event;
use futures::StreamExt;
use shannon_core::query_engine::QueryEvent;

/// Encode one engine event as an SSE frame.
///
/// The event name — and the §P3-4 serialization-failure fallback — come from
/// the shared mapping in `shannon-core::query_engine::sse`, backed by
/// `shannon_api_protocol::SseEventName`. This server previously bucketed
/// every event into `text` / `completed` / `error` / `event`, contradicting
/// `api_server`'s per-variant names on the same `QueryEvent` stream (review
/// §P2-6); both producers now emit the one canonical contract. That rename is
/// a breaking wire change for external SSE clients of this server.
pub fn event(value: QueryEvent) -> Event {
    let (event_type, data) = shannon_core::query_engine::sse::sse_parts_from_query_event(&value);
    Event::default().event(event_type).data(data)
}

// ── Stream termination guard (T4) ───────────────────────────────────────

/// SSE `event:` name for transport-level errors: `SseEventName::Error` in
/// the wire contract. The literal mirrors the serialization-failure
/// fallback in `routes::post_message` (the protocol crate is only a
/// dev-dependency here); the test below pins the literal to the enum.
const ERROR_EVENT_NAME: &str = "error";

/// Terminal frame payload reason for a stream cut short by server shutdown.
const SHUTDOWN_REASON: &str = "server shutting down";
/// Terminal frame payload reason for a stream that hit its duration cap.
const CAP_REASON: &str = "stream duration cap exceeded";

/// The terminal SSE frame for a stream that ends without its query
/// completing — server shutdown (T4) or the overall stream duration cap.
///
/// Uses the contract's transport-level `error` event with the `{"error": …}`
/// payload shape `shannon-core::query_engine::sse` and `api_server` already
/// use for stream errors (§P3-4), so existing clients surface the reason
/// without new handling and can distinguish a deliberate end from a crash.
pub fn terminal_event(reason: &str) -> Event {
    Event::default()
        .event(ERROR_EVENT_NAME)
        .data(serde_json::json!({ "error": reason }).to_string())
}

/// Wrap an SSE event stream so it cannot outlive the server (T4):
///
/// - when the shared drain flag ([`crate::AppState::shutdown`]) flips
///   `true`, emits [`terminal_event`] with [`SHUTDOWN_REASON`] and ends
///   cleanly — SIGTERM no longer cuts clients off mid-frame;
/// - when `cap` has elapsed since stream start, emits [`terminal_event`]
///   with [`CAP_REASON`] and ends cleanly — a wedged query can no longer
///   hold an SSE connection (and its keepalive pings) open forever. The cap
///   is an overall stream duration bound, NOT a per-request timeout: the
///   production value is far above any legitimate turn.
///
/// A stream that ends on its own (query completed + restore flush) passes
/// through untouched — no extra frame, same ordering. On expiry the
/// end-of-stream restore flush may be skipped (best-effort write-back).
pub fn with_shutdown_and_cap<S>(
    inner: S,
    shutdown: tokio::sync::watch::Receiver<bool>,
    cap: std::time::Duration,
) -> impl futures::Stream<Item = Result<Event, std::convert::Infallible>>
where
    S: futures::Stream<Item = Result<Event, std::convert::Infallible>>,
{
    /// One unfold state step: the live inner stream (boxed so no `Unpin`
    /// bound leaks to callers) plus the cap timer started at stream begin.
    enum GuardState<S> {
        Streaming {
            inner: std::pin::Pin<Box<S>>,
            cap_timer: std::pin::Pin<Box<tokio::time::Sleep>>,
            shutdown: tokio::sync::watch::Receiver<bool>,
        },
        Done,
    }

    let inner = Box::pin(inner);
    futures::stream::unfold(
        GuardState::Streaming {
            inner,
            cap_timer: Box::pin(tokio::time::sleep(cap)),
            shutdown,
        },
        // `move` closure: `cap` is logged inside, so it must be owned by the
        // closure (the returned stream outlives this function's frame).
        move |state| async move {
            match state {
                GuardState::Done => None,
                GuardState::Streaming {
                    mut inner,
                    mut cap_timer,
                    mut shutdown,
                } => {
                    // A closed channel (no sender left) means this router was
                    // built without a serve loop: shutdown can never fire.
                    // (`changed` + short-lived `borrow`, because `wait_for`
                    // holds a !Send guard across awaits.)
                    let shutdown_fired = async {
                        loop {
                            if shutdown.changed().await.is_err() {
                                std::future::pending::<()>().await;
                            }
                            if *shutdown.borrow() {
                                break;
                            }
                        }
                    };
                    tokio::select! {
                        // `Pin<Box<S>>` is Unpin, so StreamExt::next works on
                        // it directly (no temporary pin projections).
                        item = inner.next() => match item {
                            Some(item) => {
                                Some((item, GuardState::Streaming { inner, cap_timer, shutdown }))
                            }
                            // Natural end: pass through, no terminal frame.
                            None => None,
                        },
                        _ = &mut cap_timer => {
                            tracing::warn!(?cap, "SSE stream hit its duration cap; ending cleanly");
                            Some((Ok(terminal_event(CAP_REASON)), GuardState::Done))
                        }
                        _ = shutdown_fired => {
                            Some((Ok(terminal_event(SHUTDOWN_REASON)), GuardState::Done))
                        }
                    }
                }
            }
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::response::Sse;
    use axum::routing::get;
    use futures::stream;
    use shannon_api_protocol::SseEventName;
    use shannon_core::query_engine::sse::representative_events;
    use std::convert::Infallible;
    use tower::ServiceExt;

    /// Run the representative fixture through THIS server's real SSE encoder
    /// (axum `Event` → HTTP response bytes) and return each frame's `event:`
    /// name, in order.
    async fn wire_event_names() -> Vec<String> {
        let frames: Vec<Result<Event, Infallible>> = representative_events()
            .into_iter()
            .map(|e| Ok(event(e)))
            .collect();
        let app = axum::Router::new().route(
            "/sse",
            get(move || async move { Sse::new(stream::iter(frames)) }),
        );
        let response = app
            .oneshot(Request::builder().uri("/sse").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        text.split("\n\n")
            .filter(|frame| !frame.is_empty())
            .map(|frame| {
                frame
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .unwrap_or_else(|| panic!("SSE frame without an event name: {frame:?}"))
                    .to_string()
            })
            .collect()
    }

    /// §P2-6 divergence guard: the headless server's SSE wire output must
    /// carry the exact canonical names from `shannon_api_protocol`, one per
    /// `QueryEvent` variant — identical to what `api_server` emits for the
    /// same stream (its mapping is the same shared function). If this test
    /// fails, someone reintroduced a local name mapping instead of extending
    /// the contract.
    #[tokio::test]
    async fn sse_wire_names_are_the_canonical_contract_for_every_variant() {
        let names = wire_event_names().await;

        // Same events the api_server-side mapping is tested against: both
        // producers must agree frame by frame.
        let shared: Vec<&'static str> = representative_events()
            .iter()
            .map(shannon_core::query_engine::sse::sse_parts_from_query_event)
            .map(|(name, _)| name)
            .collect();
        let wire: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(
            wire, shared,
            "shannon-server diverged from the shared SSE mapping"
        );

        // And the names are exactly the contracted set — no legacy bucket
        // ("event"), no duplicates, nothing outside the protocol crate.
        let mut produced = wire.to_vec();
        produced.sort_unstable();
        let mut contract: Vec<&str> = [
            SseEventName::Started,
            SseEventName::Text,
            SseEventName::ToolUseRequest,
            SseEventName::ToolUseResult,
            SseEventName::TurnCompleted,
            SseEventName::Completed,
            SseEventName::Failed,
            SseEventName::Warning,
            SseEventName::Progress,
            SseEventName::ToolProgress,
            SseEventName::Thinking,
            SseEventName::Usage,
            SseEventName::Cost,
            SseEventName::Info,
            SseEventName::ConversationUpdate,
            SseEventName::RateLimit,
        ]
        .iter()
        .map(|n| n.as_str())
        .collect();
        contract.sort_unstable();
        assert_eq!(
            produced, contract,
            "SSE event names must be exactly the SseEventName contract"
        );
    }

    // ── T4: stream termination guard (shutdown + duration cap) ──────────

    /// The production terminal frames use the literal "error"; pin it to the
    /// protocol enum so a rename in `shannon_api_protocol` breaks this test
    /// instead of silently diverging the wire.
    #[test]
    fn error_event_literal_matches_the_protocol_contract() {
        assert_eq!(ERROR_EVENT_NAME, SseEventName::Error.as_str());
    }

    /// Render an SSE stream through the real axum encoder and return each
    /// frame as `(event name, data payload)`.
    async fn render_sse_body<S>(stream: S) -> Vec<(String, String)>
    where
        S: futures::Stream<Item = Result<Event, Infallible>> + Send + 'static,
    {
        // Rendering via `IntoResponse` skips the Router/Handler plumbing —
        // axum handlers must be `Clone`, which a bare stream is not.
        use axum::response::IntoResponse;
        let response = Sse::new(stream).into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec())
            .unwrap()
            .split("\n\n")
            .filter(|frame| !frame.is_empty())
            .map(|frame| {
                let event_name = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .unwrap_or_else(|| panic!("SSE frame without an event name: {frame:?}"))
                    .to_string();
                let data = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .unwrap_or_default()
                    .to_string();
                (event_name, data)
            })
            .collect()
    }

    /// A stream that yields `events` then never ends — the "wedged query"
    /// the duration cap exists for.
    fn wedged_after(
        events: Vec<Result<Event, Infallible>>,
    ) -> impl futures::Stream<Item = Result<Event, Infallible>> {
        use futures::StreamExt;
        let forever_pending = futures::stream::unfold((), |()| async {
            std::future::pending::<Option<(Result<Event, Infallible>, ())>>().await
        });
        futures::stream::iter(events).chain(forever_pending)
    }

    fn text_event(content: &str) -> Result<Event, Infallible> {
        Ok(Event::default().event("text").data(content))
    }

    #[tokio::test]
    async fn duration_cap_emits_terminal_error_and_ends_the_stream() {
        let (_tx, shutdown) = tokio::sync::watch::channel(false);
        let inner = wedged_after(vec![text_event("partial output")]);
        let guarded =
            super::with_shutdown_and_cap(inner, shutdown, std::time::Duration::from_millis(25));
        let frames =
            tokio::time::timeout(std::time::Duration::from_secs(5), render_sse_body(guarded))
                .await
                .expect("the guarded stream must end despite the wedged inner stream");
        assert_eq!(
            frames,
            vec![
                ("text".to_string(), "partial output".to_string()),
                (
                    "error".to_string(),
                    serde_json::json!({"error": "stream duration cap exceeded"}).to_string()
                )
            ],
            "the cap must deliver the buffered events, then exactly one terminal frame"
        );
    }

    #[tokio::test]
    async fn shutdown_flag_emits_terminal_error_and_ends_the_stream() {
        let (tx, shutdown) = tokio::sync::watch::channel(false);
        let inner = wedged_after(vec![text_event("mid-turn")]);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            let _ = tx.send(true);
        });
        let guarded =
            super::with_shutdown_and_cap(inner, shutdown, std::time::Duration::from_secs(60));
        let frames =
            tokio::time::timeout(std::time::Duration::from_secs(5), render_sse_body(guarded))
                .await
                .expect("the guarded stream must end once the drain flag flips");
        let last = frames.last().expect("at least one frame");
        assert_eq!(last.0, "error", "shutdown must end with the terminal event");
        assert!(
            last.1.contains("server shutting down"),
            "terminal payload must name the reason: {last:?}"
        );
        assert_eq!(
            frames.len(),
            2,
            "exactly one buffered event then the terminal frame"
        );
    }

    #[tokio::test]
    async fn stream_that_finishes_on_its_own_gets_no_terminal_frame() {
        let (_tx, shutdown) = tokio::sync::watch::channel(false);
        let inner = futures::stream::iter(vec![
            text_event("hello"),
            Ok(Event::default().event("completed").data("{}")),
        ]);
        let guarded =
            super::with_shutdown_and_cap(inner, shutdown, std::time::Duration::from_secs(60));
        let frames = render_sse_body(guarded).await;
        assert_eq!(
            frames,
            vec![
                ("text".to_string(), "hello".to_string()),
                ("completed".to_string(), "{}".to_string())
            ],
            "a naturally-ending stream passes through untouched"
        );
    }
}
