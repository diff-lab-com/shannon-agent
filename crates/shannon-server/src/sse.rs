use axum::response::sse::Event;
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
}
