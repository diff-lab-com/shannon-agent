use axum::response::sse::Event;
use shannon_core::query_engine::QueryEvent;

pub fn event(value: QueryEvent) -> Event {
    Event::default()
        .event(match &value {
            QueryEvent::Text { .. } => "text",
            QueryEvent::Completed { .. } => "completed",
            QueryEvent::Failed { .. } => "error",
            _ => "event",
        })
        .json_data(value)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"))
}
